//! HTTP server, auth, relay control, and status handlers for REFEREE.

use axum::{
    extract::{ConnectInfo, Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::{debug, error, info, trace, warn};

use crate::gpu::GpuInfo;
use crate::graph::{
    parse_resolution, BackendCapabilities, ExecutionPlan, ExecutorKind, GraphPlanner, HdrRequest,
    InterpolationDecision, InterpolationRealization, InterpolationRequest,
    InterpolationUnsupportedReason, LatencyMode, PipelineRequest, UpscaleRequest, VideoOp,
};
use crate::pipeline::{
    self, clamp_quality, cleanup_session, get_encoding_profile, start_pipeline,
    wait_for_packager_playlist, EncoderCapabilities, EncodingProfile, ExecutorPreference,
    ExecutorSelectionContext, FramegenEngine, FramegenSettings, FramegenTarget,
    PipelineCompletionSignal, PipelineLaunch, PlaylistWaitOutcome, RemoteSessionBacking,
    RuntimeBackend, Session, SessionInfo, SessionMap, HEARTBEAT_TIMEOUT_MS,
    ORPHAN_SESSION_TIMEOUT_MS,
};
use crate::settings::{
    ApprovedOriginMeta, RelayPeerMetadata, RelaySettings, Settings, SettingsManager,
};
use crate::source::hls_relay;
use crate::source::{
    describe_source, SourceContentKind, SourceDescriptor, SourceKind, SourceMetadata,
    SourceTransport,
};

#[cfg(not(feature = "headless"))]
use tauri::{AppHandle as DesktopAppHandle, Emitter, Manager};

#[cfg(feature = "headless")]
type DesktopAppHandle = ();

mod auth;
mod clients;
mod relay;
mod responses;
mod routes;
mod state;
mod status;
mod stream;

#[cfg(test)]
mod tests;

use auth::{
    handle_add_origin, handle_auth_request, handle_delete_origin, handle_list_origins,
    handle_rotate_token, handle_stream_settings_update, require_api_token,
};
pub use auth::{load_approved_origins, persist_approved_origins};
pub(crate) use clients::validate_http_clients;
use clients::{relay_control_http_client, remote_tmp_proxy_http_client};
use relay::*;
#[cfg(not(feature = "headless"))]
pub(crate) use relay::{adopt_moved_relay_peer, linked_relay_peer, RelayLinkFreshness};
pub use relay::{stop_tracked_session, RelayLinkMonitor};
use responses::*;
pub use routes::{shutdown, start_server};
pub(crate) use state::RateLimiter;
pub use state::{ConsentDecision, ServerSettings, ServerState};
use state::{LocalStreamSettings, RemoteIp, StreamUiSettings};
pub(crate) use status::{build_status, finalize_status};
use status::{emit_status_update, handle_ping, handle_status};
use stream::*;

const PORT: u16 = 14002;
const INPUT_PROXY_HOST: &str = "127.0.0.1";
const LIVE_PLAYLIST_CACHE_CONTROL: &str =
    "no-store, no-cache, must-revalidate, proxy-revalidate, max-age=0";
const HLS_SEGMENT_CACHE_CONTROL: &str = "public, max-age=60";
// Relay probes should fail fast so status polling stays responsive.
const RELAY_STATUS_TIMEOUT_MS: u64 = 1_500;
// Peers share a LAN, so a connect that has not completed by now means the peer is offline.
const RELAY_CONNECT_TIMEOUT_MS: u64 = 1_500;
// Bounds a peer that stalls mid-segment on the playback proxy.
const RELAY_TMP_PROXY_READ_TIMEOUT_SECS: u64 = 10;
// Stream startup can include remote source probing, setup, and first playlist generation.
const RELAY_STREAM_START_TIMEOUT_SECS: u64 = 195;
// Session status/control calls are user-facing follow-ups and should remain snappy.
const RELAY_SESSION_STATUS_TIMEOUT_SECS: u64 = 5;
const RELAY_SESSION_CONTROL_TIMEOUT_SECS: u64 = 5;
// Live processing stats are refreshed every second while a relay session is open.
const RELAY_LIVE_STATS_TIMEOUT_MS: u64 = 2_000;
// The start response is already in hand, so a transient status-sync miss is retried
// before the remote session is rolled back.
const RELAY_STATUS_SYNC_ATTEMPTS: u32 = 3;
const RELAY_STATUS_SYNC_RETRY_DELAY_MS: u64 = 250;
// The peer reaps a session 15 s after its last heartbeat, so a 3 s keepalive leaves
// several attempts per window for a slow or dropped request.
const RELAY_KEEPALIVE_INTERVAL_MS: u64 = 3_000;
// Past this long without a successful keepalive the peer has reaped the session itself.
const RELAY_PEER_LOSS_TIMEOUT_MS: u64 = 20_000;
// Status polling reuses a link probe this young instead of re-probing the peer.
const RELAY_LINK_PROBE_TTL_MS: u64 = 3_000;
// Minimum gap between background subnet scans for a linked peer whose address changed.
const RELAY_REBIND_SCAN_INTERVAL_SECS: u64 = 30;
// Per-host timeout for a subnet scan (matches desktop discovery).
const RELAY_DISCOVERY_TIMEOUT_MS: u64 = 1_000;
#[cfg(not(feature = "headless"))]
pub(crate) const CONSENT_REQUEST_TIMEOUT_SECS: u64 = 180;

mod error_codes {
    #[cfg(not(feature = "headless"))]
    pub const CONSENT_DENIED: &str = "CONSENT_DENIED";
    #[cfg(not(feature = "headless"))]
    pub const CONSENT_TIMEOUT: &str = "CONSENT_TIMEOUT";
    #[cfg(feature = "headless")]
    pub const HEADLESS_MODE: &str = "HEADLESS_MODE";
    pub const INVALID_HEADERS: &str = "INVALID_HEADERS";
    pub const INVALID_ORIGIN: &str = "INVALID_ORIGIN";
    pub const INVALID_REQUEST: &str = "INVALID_REQUEST";
    pub const INVALID_SETTINGS: &str = "INVALID_SETTINGS";
    pub const INVALID_URL: &str = "INVALID_URL";
    pub const MISSING_ORIGIN: &str = "MISSING_ORIGIN";
    #[cfg(not(feature = "headless"))]
    pub const NO_APP_HANDLE: &str = "NO_APP_HANDLE";
    pub const NO_ENCODER: &str = "NO_ENCODER";
    pub const ORIGIN_NOT_FOUND: &str = "ORIGIN_NOT_FOUND";
    pub const PIPELINE_EXITED: &str = "PIPELINE_EXITED";
    pub const PIPELINE_TIMEOUT: &str = "PIPELINE_TIMEOUT";
    pub const RATE_LIMITED: &str = "RATE_LIMITED";
    pub const RELAY_REQUIRED: &str = "RELAY_REQUIRED";
    pub const RELAY_START_FAILED: &str = "RELAY_START_FAILED";
    pub const RELAY_STATUS_SYNC_FAILED: &str = "RELAY_STATUS_SYNC_FAILED";
    pub const RELAY_UNAVAILABLE: &str = "RELAY_UNAVAILABLE";
    pub const SESSION_NOT_FOUND: &str = "SESSION_NOT_FOUND";
    pub const SSRF_BLOCKED: &str = "SSRF_BLOCKED";
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
}

pub fn local_ip() -> std::net::IpAddr {
    // Probe the local outbound IP by connecting a UDP socket (no packets sent).
    // Fall back to loopback if the network is unavailable or the OS rejects the probe.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|addr| addr.ip())
        .unwrap_or_else(|error| {
            warn!(
                "[Main]: Could not determine LAN IP, falling back to localhost: {}",
                error
            );
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        })
}
