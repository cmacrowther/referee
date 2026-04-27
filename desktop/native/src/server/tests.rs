use super::{
    assemble_startup_context, build_pipeline_request, build_remote_tmp_proxy_url,
    build_remote_tmp_target_url, build_status, enrich_status_with_remote_processing_stats,
    fetch_remote_session_info, forward_remote_heartbeat_request, forward_remote_stream_start,
    local_output_url, stop_tracked_session, tmp_file_response_headers, RateLimiter,
    RelayControlTarget, RuntimeBackend, ServerSettings, StopRequest, StreamStartRequest,
};
use crate::gpu::GpuInfo;
use crate::graph::{
    BackendCapabilities, ExecutorKind, HdrRequest, HdrSupport, InterpolationRealization,
    InterpolationRequest, InterpolationSupport, InterpolationUnsupportedReason,
    NativeUpscaleBackend, ResizeSupport, UpscalePlanningCapabilities, UpscaleRequest, VideoOp,
};
use crate::pipeline::{
    new_session_map, EncoderCapabilities, EncodingProfile, ExecutorPreference,
    PipelineCompletionSignal, RemoteSessionBacking, Session, SessionInfo,
};
use crate::settings::{ApprovedOriginMeta, RelayPeerMetadata, SettingsManager};
use crate::source::{
    SourceClassification, SourceContentKind, SourceDescriptor, SourceKind, SourceMetadata,
    SourceTransport,
};
use axum::{
    extract::{Path as AxumPath, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};

fn settings(resolution: &str, framegen: bool) -> ServerSettings {
    ServerSettings {
        resolution: resolution.to_string(),
        quality: 3,
        framegen,
        hdr: false,
        executor_preference: ExecutorPreference::Auto,
        show_on_proxy_start: false,
        notifications: false,
        encoding_profiles: HashMap::<String, EncodingProfile>::new(),
        relay: crate::settings::RelaySettings::default(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ObservedRelayRequest {
    path: String,
    token: Option<String>,
    body: Option<serde_json::Value>,
}

#[derive(Clone)]
struct RelayMockState {
    requests: Arc<Mutex<Vec<ObservedRelayRequest>>>,
    start_status: axum::http::StatusCode,
    start_body: serde_json::Value,
    stop_status: axum::http::StatusCode,
    stop_body: serde_json::Value,
    heartbeat_status: axum::http::StatusCode,
    heartbeat_body: serde_json::Value,
    status_status: axum::http::StatusCode,
    status_body: serde_json::Value,
}

async fn spawn_relay_mock_server(state: RelayMockState) -> (String, tokio::task::JoinHandle<()>) {
    async fn start_handler(
        State(state): State<RelayMockState>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        state.requests.lock().unwrap().push(ObservedRelayRequest {
            path: "/v1/stream/start".to_string(),
            token: headers
                .get("X-Referee-Token")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: Some(body),
        });

        (state.start_status, Json(state.start_body.clone()))
    }

    async fn stop_handler(
        State(state): State<RelayMockState>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        state.requests.lock().unwrap().push(ObservedRelayRequest {
            path: "/v1/stream/stop".to_string(),
            token: headers
                .get("X-Referee-Token")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: Some(body),
        });

        (state.stop_status, Json(state.stop_body.clone()))
    }

    async fn heartbeat_handler(
        State(state): State<RelayMockState>,
        headers: HeaderMap,
        AxumPath(session_id): AxumPath<String>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        state.requests.lock().unwrap().push(ObservedRelayRequest {
            path: format!("/v1/stream/heartbeat/{}", session_id),
            token: headers
                .get("X-Referee-Token")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: None,
        });

        (state.heartbeat_status, Json(state.heartbeat_body.clone()))
    }

    async fn status_handler(
        State(state): State<RelayMockState>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        (state.status_status, Json(state.status_body.clone()))
    }

    let app = Router::new()
        .route("/v1/stream/start", post(start_handler))
        .route("/v1/stream/stop", post(stop_handler))
        .route("/v1/stream/heartbeat/{session_id}", post(heartbeat_handler))
        .route("/v1/status", get(status_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{}", address), handle)
}

/// Creates a default Windows RuntimeBackend configured for NVIDIA NVENC tools.
///
/// The returned backend is prepopulated with `gpu_vendor = "nvidia"`, `encoder_backend = "nvenc"`,
/// `encoder_path = "NVEncC64.exe"`, `ffmpeg_path = "ffmpeg.exe"`, and leaves RIFE-related paths as `None`.
///
/// # Examples
///
fn runtime_backend() -> RuntimeBackend {
    RuntimeBackend {
        gpu_vendor: "nvidia".to_string(),
        encoder_backend: "nvenc".to_string(),
        encoder_path: PathBuf::from("NVEncC64.exe"),
        ffmpeg_path: PathBuf::from("ffmpeg.exe"),
        rife_worker_path: None,
        rife_model_path: None,
    }
}

fn amd_runtime_backend() -> RuntimeBackend {
    RuntimeBackend {
        gpu_vendor: "amd".to_string(),
        encoder_backend: "vceenc".to_string(),
        encoder_path: PathBuf::from("VCEEncC64.exe"),
        ffmpeg_path: PathBuf::from("ffmpeg.exe"),
        rife_worker_path: None,
        rife_model_path: None,
    }
}

fn source_descriptor(
    transport: SourceTransport,
    kind: SourceKind,
    content_kind: SourceContentKind,
    resolution: Option<&str>,
    fps: Option<f64>,
    width: Option<u32>,
    height: Option<u32>,
) -> SourceDescriptor {
    SourceDescriptor {
        classification: SourceClassification { transport, kind },
        original_url: "https://example.com/live/master.m3u8".to_string(),
        runtime_url: "http://127.0.0.1:14002/v1/input/session-1?url=...".to_string(),
        runtime_headers: HashMap::new(),
        session_headers: HashMap::new(),
        relay: None,
        metadata: Some(SourceMetadata {
            width,
            height,
            source_resolution: resolution.map(ToOwned::to_owned),
            source_fps: fps,
            content_kind,
            content_kind_confidence: Some(0.9),
        }),
    }
}

/// Constructs a BackendCapabilities value configured for an NVIDIA specialized executor.
///
/// The returned capabilities indicate:
/// - `ExecutorKind::NvidiaSpecialized` executor,
/// - quality-range resize support (quality 1–4),
/// - interpolation capability to 60 FPS,
/// - HDR support that enables tonemapping to HDR10 (no 10-bit passthrough or HDR10 metadata injection),
/// - native backend upscaling via `NativeUpscaleBackend::NvidiaNgxVsr`.
///
/// # Examples
///
fn planner_capabilities() -> BackendCapabilities {
    BackendCapabilities {
        executor: ExecutorKind::NvidiaSpecialized,
        resize: ResizeSupport::QualityRange {
            min_quality: 1,
            max_quality: 4,
        },
        interpolation: InterpolationSupport::To60,
        hdr: HdrSupport {
            passthrough_10_bit: false,
            tonemap_to_hdr10: true,
            inject_hdr10_metadata: false,
        },
        upscale: UpscalePlanningCapabilities::native_backend(NativeUpscaleBackend::NvidiaNgxVsr),
    }
}

/// Default encoder capability flags used when runtime detection is unavailable.
///
/// Produces an `EncoderCapabilities` instance with conservative defaults:
/// VPP resize, FRUC, and TrueHDR enabled; RIFE and streaming RIFE disabled.
///
/// # Examples
///
fn encoder_capabilities() -> EncoderCapabilities {
    EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
    }
}

/// Produces a GpuInfo populated with representative NVIDIA RTX 4080 values.
///
/// This provides a default/mock GPU configuration (vendor, name, encoder/backend paths, and basic utilization)
/// intended for testing or local default usage.
///
/// # Examples
///
fn gpu_info() -> GpuInfo {
    GpuInfo {
        vendor: "nvidia".to_string(),
        name: Some("NVIDIA GeForce RTX 4080".to_string()),
        backend: Some("nvenc".to_string()),
        encoder_path: Some(PathBuf::from("NVEncC64.exe")),
        ffmpeg_path: Some(PathBuf::from("ffmpeg.exe")),
        rife_worker_path: None,
        rife_model_path: None,
        utilization: Some(0),
    }
}

fn insert_session_with_executor(sessions: &crate::pipeline::SessionMap, executor: ExecutorKind) {
    let (heartbeat_tx, _heartbeat_rx) = mpsc::channel(1);
    sessions.insert(
        "session-1".to_string(),
        Session::new_local(
            SessionInfo {
                id: "session-1".to_string(),
                source_url: "https://example.com/live/master.m3u8".to_string(),
                output_url: "http://127.0.0.1:14002/v1/tmp/session-1/index.m3u8".to_string(),
                app_name: None,
                stream_title: None,
                source_content_kind: Some(SourceContentKind::Animated),
                upscaler: Some("Anime4K2x".to_string()),
                source_resolution: Some("1920x1080".to_string()),
                output_resolution: "3840x2160".to_string(),
                source_fps: Some(30.0),
                target_fps: Some(60.0),
                framegen_enabled: executor == ExecutorKind::NvidiaSpecialized,
                hdr_enabled: true,
                quality_level: 3,
                executor,
                encoder_backend: Some("nvenc".to_string()),
                startup_complete: true,
                retrying_startup: false,
                startup_stage: "ready".to_string(),
            },
            HashMap::new(),
            heartbeat_tx,
            PathBuf::from("session-1"),
            PipelineCompletionSignal::new(),
            1,
            "low-latency",
        ),
    );
}

#[test]
fn tmp_file_headers_mark_live_playlist_uncacheable() {
    let headers = tmp_file_response_headers("index.m3u8");

    assert_eq!(headers.content_type, "application/vnd.apple.mpegurl");
    assert!(headers.cache_control.contains("no-store"));
    assert!(headers.cache_control.contains("max-age=0"));
    assert!(headers.legacy_no_cache);
}

#[test]
fn tmp_file_headers_mark_segments_as_transport_stream() {
    let headers = tmp_file_response_headers("segment_000001.ts");

    assert_eq!(headers.content_type, "video/mp2t");
    assert_eq!(headers.cache_control, "public, max-age=60");
    assert!(!headers.legacy_no_cache);
}

#[test]
fn remote_tmp_proxy_url_rewrites_back_through_localhost_tmp_route() {
    let target = reqwest::Url::parse(
        "http://192.168.1.25:14002/v1/tmp/remote-session-1/chunks/segment0.ts?token=abc",
    )
    .unwrap();
    let proxy_url = build_remote_tmp_proxy_url("local-session-1", &target);

    assert_eq!(
            proxy_url,
            "http://localhost:14002/v1/tmp/local-session-1/segment0.ts?url=http%3A%2F%2F192.168.1.25%3A14002%2Fv1%2Ftmp%2Fremote-session-1%2Fchunks%2Fsegment0.ts%3Ftoken%3Dabc"
        );
}

#[test]
fn remote_tmp_target_url_rejects_requests_outside_the_remote_session_prefix() {
    let remote = RemoteSessionBacking {
        remote_base_url: "http://192.168.1.25:14002".to_string(),
        remote_session_id: "remote-session-1".to_string(),
        remote_token: "relay-secret".to_string(),
        selected_peer: RelayPeerMetadata {
            instance_id: Some("peer-1".to_string()),
            hostname: Some("media-box".to_string()),
            ip: Some("192.168.1.25".to_string()),
            version: Some("1.2.3".to_string()),
            platform: Some("linux".to_string()),
            gpu_ready: Some(true),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_name: Some("RTX 4080".to_string()),
        },
    };

    let error = build_remote_tmp_target_url(
        &remote,
        "index.m3u8",
        Some("http://192.168.1.25:14002/v1/tmp/other-session/index.m3u8"),
    )
    .unwrap_err();

    assert_eq!(error, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn forward_remote_stream_start_sends_token_and_parses_response() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (base_url, handle) = spawn_relay_mock_server(RelayMockState {
        requests: requests.clone(),
        start_status: axum::http::StatusCode::OK,
        start_body: json!({
            "sessionId": "remote-session-1",
            "url": "http://localhost:14002/v1/tmp/remote-session-1/index.m3u8",
            "resolution": "3840x2160",
            "sourceResolution": "1920x1080",
            "appName": "Remote App",
            "streamTitle": "Remote Stream",
            "effectiveQuality": 4,
            "evictedSessions": []
        }),
        stop_status: axum::http::StatusCode::OK,
        stop_body: json!({ "status": "stopped" }),
        heartbeat_status: axum::http::StatusCode::OK,
        heartbeat_body: json!({ "status": "ok" }),
        status_status: axum::http::StatusCode::OK,
        status_body: json!({ "sessions": [], "primarySession": null }),
    })
    .await;

    let response = forward_remote_stream_start(
        &StreamStartRequest {
            url: "https://example.com/live/master.m3u8".to_string(),
            app_name: Some("Desktop App".to_string()),
            stream_title: Some("Friday Night".to_string()),
            headers: Some(HashMap::from([(
                "authorization".to_string(),
                "Bearer demo".to_string(),
            )])),
            content_kind: Some(SourceContentKind::Animated),
        },
        &RelayControlTarget {
            base_url,
            remote_token: "relay-secret".to_string(),
            selected_peer: RelayPeerMetadata::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(response.session_id, "remote-session-1");
    assert_eq!(response.effective_quality, 4);

    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/v1/stream/start");
    assert_eq!(seen[0].token.as_deref(), Some("relay-secret"));
    assert_eq!(
        seen[0].body.as_ref().unwrap()["headers"]["authorization"],
        "Bearer demo"
    );

    handle.abort();
}

#[tokio::test]
async fn fetch_remote_session_info_returns_matching_session() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (base_url, handle) = spawn_relay_mock_server(RelayMockState {
        requests,
        start_status: axum::http::StatusCode::OK,
        start_body: json!({}),
        stop_status: axum::http::StatusCode::OK,
        stop_body: json!({ "status": "stopped" }),
        heartbeat_status: axum::http::StatusCode::OK,
        heartbeat_body: json!({ "status": "ok" }),
        status_status: axum::http::StatusCode::OK,
        status_body: json!({
            "sessions": [
                {
                    "id": "remote-session-1",
                    "sourceUrl": "https://example.com/live/master.m3u8",
                    "outputUrl": "http://127.0.0.1:14002/v1/tmp/remote-session-1/index.m3u8",
                    "appName": "Remote App",
                    "streamTitle": "Remote Stream",
                    "sourceContentKind": "animated",
                    "upscaler": "Anime4K2x",
                    "sourceResolution": "1920x1080",
                    "outputResolution": "3840x2160",
                    "sourceFps": 30.0,
                    "targetFps": 60.0,
                    "framegenEnabled": true,
                    "hdrEnabled": true,
                    "qualityLevel": 4,
                    "executor": "nvidiaSpecialized",
                    "encoderBackend": "nvenc",
                    "startupComplete": true,
                    "retryingStartup": false,
                    "startupStage": "ready"
                }
            ],
            "primarySession": null
        }),
    })
    .await;

    let session = fetch_remote_session_info(
        &RelayControlTarget {
            base_url,
            remote_token: "relay-secret".to_string(),
            selected_peer: RelayPeerMetadata::default(),
        },
        "remote-session-1",
    )
    .await
    .unwrap();

    assert_eq!(session.id, "remote-session-1");
    assert_eq!(session.upscaler.as_deref(), Some("Anime4K2x"));
    assert_eq!(session.encoder_backend.as_deref(), Some("nvenc"));

    handle.abort();
}

#[tokio::test]
async fn forward_remote_heartbeat_request_returns_false_for_missing_session() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (base_url, handle) = spawn_relay_mock_server(RelayMockState {
        requests: requests.clone(),
        start_status: axum::http::StatusCode::OK,
        start_body: json!({}),
        stop_status: axum::http::StatusCode::OK,
        stop_body: json!({ "status": "stopped" }),
        heartbeat_status: axum::http::StatusCode::NOT_FOUND,
        heartbeat_body: json!({
            "code": "SESSION_NOT_FOUND",
            "error": "Session not found"
        }),
        status_status: axum::http::StatusCode::OK,
        status_body: json!({ "sessions": [], "primarySession": null }),
    })
    .await;

    let heartbeat = forward_remote_heartbeat_request(&RemoteSessionBacking {
        remote_base_url: base_url,
        remote_session_id: "remote-session-1".to_string(),
        remote_token: "relay-secret".to_string(),
        selected_peer: RelayPeerMetadata::default(),
    })
    .await
    .unwrap();

    assert!(!heartbeat);
    let seen = requests.lock().unwrap();
    assert_eq!(seen[0].path, "/v1/stream/heartbeat/remote-session-1");
    assert_eq!(seen[0].token.as_deref(), Some("relay-secret"));

    handle.abort();
}

#[tokio::test]
async fn stop_tracked_session_for_remote_session_forwards_stop_and_cleans_up() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (base_url, handle) = spawn_relay_mock_server(RelayMockState {
        requests: requests.clone(),
        start_status: axum::http::StatusCode::OK,
        start_body: json!({}),
        stop_status: axum::http::StatusCode::OK,
        stop_body: json!({ "status": "stopped" }),
        heartbeat_status: axum::http::StatusCode::OK,
        heartbeat_body: json!({ "status": "ok" }),
        status_status: axum::http::StatusCode::OK,
        status_body: json!({ "sessions": [], "primarySession": null }),
    })
    .await;

    let sessions = new_session_map();
    sessions.insert(
        "local-session-1".to_string(),
        Session::new_remote(
            SessionInfo {
                id: "local-session-1".to_string(),
                source_url: "https://example.com/live/master.m3u8".to_string(),
                output_url: "http://localhost:14002/v1/tmp/local-session-1/index.m3u8".to_string(),
                app_name: Some("Remote App".to_string()),
                stream_title: Some("Remote Stream".to_string()),
                source_content_kind: Some(SourceContentKind::Animated),
                upscaler: Some("Anime4K2x".to_string()),
                source_resolution: Some("1920x1080".to_string()),
                output_resolution: "3840x2160".to_string(),
                source_fps: Some(30.0),
                target_fps: Some(60.0),
                framegen_enabled: true,
                hdr_enabled: true,
                quality_level: 4,
                executor: ExecutorKind::NvidiaSpecialized,
                encoder_backend: Some("nvenc".to_string()),
                startup_complete: true,
                retrying_startup: false,
                startup_stage: "ready".to_string(),
            },
            base_url,
            "remote-session-1".to_string(),
            "relay-secret".to_string(),
            RelayPeerMetadata::default(),
        ),
    );

    let stopped_app_name = stop_tracked_session("local-session-1", &sessions)
        .await
        .unwrap();

    assert_eq!(stopped_app_name.as_deref(), Some("Remote App"));
    assert!(sessions.is_empty());

    let seen = requests.lock().unwrap();
    assert_eq!(seen[0].path, "/v1/stream/stop");
    assert_eq!(seen[0].token.as_deref(), Some("relay-secret"));
    assert_eq!(
        serde_json::from_value::<StopRequest>(seen[0].body.clone().unwrap())
            .unwrap()
            .session_id
            .as_deref(),
        Some("remote-session-1")
    );

    handle.abort();
}

#[tokio::test]
async fn enriched_status_uses_remote_processing_stats_for_relay_session() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (base_url, handle) = spawn_relay_mock_server(RelayMockState {
        requests,
        start_status: axum::http::StatusCode::OK,
        start_body: json!({}),
        stop_status: axum::http::StatusCode::OK,
        stop_body: json!({ "status": "stopped" }),
        heartbeat_status: axum::http::StatusCode::OK,
        heartbeat_body: json!({ "status": "ok" }),
        status_status: axum::http::StatusCode::OK,
        status_body: json!({
            "gpuReady": true,
            "gpuName": "Radeon RX 7900 XTX",
            "gpuVendor": "amd",
            "gpuUtilization": 82,
            "encoderBackend": "vceenc",
            "selectedExecutor": "universal",
            "nvidiaAiAvailable": false,
            "amdAiAvailable": true,
            "encoderHasFramegen": false,
            "encoderHasTruehdr": true,
            "encoderHasRife": true,
            "sessions": [
                {
                    "id": "remote-session-1",
                    "sourceUrl": "https://example.com/live/master.m3u8",
                    "outputUrl": "http://127.0.0.1:14002/v1/tmp/remote-session-1/index.m3u8",
                    "appName": "Remote App",
                    "streamTitle": "Remote Stream",
                    "sourceContentKind": "animated",
                    "upscaler": "ArtCNN",
                    "sourceResolution": "1280x720",
                    "outputResolution": "2560x1440",
                    "sourceFps": 24.0,
                    "targetFps": 24.0,
                    "framegenEnabled": false,
                    "hdrEnabled": true,
                    "qualityLevel": 2,
                    "executor": "universal",
                    "encoderBackend": "vceenc",
                    "startupComplete": true,
                    "retryingStartup": false,
                    "startupStage": "ready"
                }
            ],
            "primarySession": null
        }),
    })
    .await;

    let sessions = new_session_map();
    sessions.insert(
        "local-session-1".to_string(),
        Session::new_remote(
            SessionInfo {
                id: "local-session-1".to_string(),
                source_url: "https://example.com/live/master.m3u8".to_string(),
                output_url: "http://localhost:14002/v1/tmp/local-session-1/index.m3u8".to_string(),
                app_name: Some("Stale App".to_string()),
                stream_title: Some("Stale Stream".to_string()),
                source_content_kind: Some(SourceContentKind::Animated),
                upscaler: Some("Anime4K2x".to_string()),
                source_resolution: Some("1920x1080".to_string()),
                output_resolution: "3840x2160".to_string(),
                source_fps: Some(30.0),
                target_fps: Some(60.0),
                framegen_enabled: true,
                hdr_enabled: true,
                quality_level: 4,
                executor: ExecutorKind::NvidiaSpecialized,
                encoder_backend: Some("nvenc".to_string()),
                startup_complete: true,
                retrying_startup: false,
                startup_stage: "ready".to_string(),
            },
            base_url,
            "remote-session-1".to_string(),
            "relay-secret".to_string(),
            RelayPeerMetadata::default(),
        ),
    );

    let mut gpu = gpu_info();
    gpu.utilization = Some(4);
    let status = build_status(
        &gpu,
        &settings("3840x2160", true),
        &sessions,
        Some(&encoder_capabilities()),
        true,
    );
    let enriched = enrich_status_with_remote_processing_stats(status, &sessions).await;

    assert_eq!(enriched.gpu_ready, Some(true));
    assert_eq!(enriched.gpu_name.as_deref(), Some("Radeon RX 7900 XTX"));
    assert_eq!(enriched.gpu_vendor.as_deref(), Some("amd"));
    assert_eq!(enriched.gpu_utilization, Some(82));
    assert_eq!(enriched.encoder_backend.as_deref(), Some("vceenc"));
    assert_eq!(enriched.selected_executor, Some(ExecutorKind::Universal));
    assert_eq!(enriched.nvidia_ai_available, Some(false));
    assert_eq!(enriched.amd_ai_available, Some(true));
    assert_eq!(enriched.encoder_has_framegen, Some(false));
    assert_eq!(enriched.encoder_has_truehdr, Some(true));
    assert_eq!(enriched.encoder_has_rife, Some(true));

    let primary = enriched.primary_session.as_ref().unwrap();
    assert_eq!(primary.id, "local-session-1");
    assert_eq!(primary.output_url, local_output_url("local-session-1"));
    assert_eq!(primary.app_name.as_deref(), Some("Remote App"));
    assert_eq!(primary.upscaler.as_deref(), Some("ArtCNN"));
    assert_eq!(primary.encoder_backend.as_deref(), Some("vceenc"));

    handle.abort();
}

#[test]
fn status_prediction_respects_universal_executor_preference() {
    let mut settings = settings("3840x2160", true);
    settings.hdr = true;
    settings.executor_preference = ExecutorPreference::Universal;
    let sessions = new_session_map();
    let caps = encoder_capabilities();
    let gpu = gpu_info();

    let status = build_status(&gpu, &settings, &sessions, Some(&caps), true);

    assert_eq!(status.selected_executor, Some(ExecutorKind::Universal));
    assert_eq!(
        status.nvidia_ai_available,
        Some(crate::exec::RuntimePlatform::current() == crate::exec::RuntimePlatform::Windows)
    );
}

#[test]
fn status_uses_active_session_executor_over_idle_prediction() {
    let mut settings = settings("3840x2160", true);
    settings.hdr = true;
    settings.executor_preference = ExecutorPreference::Universal;
    let sessions = new_session_map();
    insert_session_with_executor(&sessions, ExecutorKind::NvidiaSpecialized);
    let caps = encoder_capabilities();
    let gpu = gpu_info();

    let status = build_status(&gpu, &settings, &sessions, Some(&caps), true);

    assert_eq!(
        status.selected_executor,
        Some(ExecutorKind::NvidiaSpecialized)
    );
}

#[tokio::test]
async fn status_reports_not_ready_when_selected_relay_peer_is_offline() {
    let mut settings = settings("3840x2160", true);
    settings.relay = crate::settings::RelaySettings {
        enabled: true,
        linked_peer_id: Some("peer-1".to_string()),
        linked_peer_hostname: Some("media-box".to_string()),
        linked_peer_ip: Some("0.0.0.0".to_string()),
        remote_token: Some("relay-secret".to_string()),
        last_known_peer: Some(RelayPeerMetadata {
            instance_id: Some("peer-1".to_string()),
            hostname: Some("media-box".to_string()),
            ip: Some("0.0.0.0".to_string()),
            version: Some("1.2.3".to_string()),
            platform: Some("linux".to_string()),
            gpu_ready: Some(true),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_name: Some("RTX 4080".to_string()),
        }),
    };
    let gpu = gpu_info();
    let sessions = new_session_map();

    let status = build_status(&gpu, &settings, &sessions, None, true);
    let status = super::apply_status_readiness(status, &gpu, &settings, true).await;

    assert_eq!(status.gpu_ready, Some(false));
}

#[tokio::test]
async fn active_relay_target_errors_when_selected_relay_peer_is_offline() {
    let mut settings = settings("3840x2160", true);
    settings.relay = crate::settings::RelaySettings {
        enabled: true,
        linked_peer_id: Some("peer-1".to_string()),
        linked_peer_hostname: Some("media-box".to_string()),
        linked_peer_ip: Some("0.0.0.0".to_string()),
        remote_token: Some("relay-secret".to_string()),
        last_known_peer: Some(RelayPeerMetadata {
            instance_id: Some("peer-1".to_string()),
            hostname: Some("media-box".to_string()),
            ip: Some("0.0.0.0".to_string()),
            version: Some("1.2.3".to_string()),
            platform: Some("linux".to_string()),
            gpu_ready: Some(true),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_name: Some("RTX 4080".to_string()),
        }),
    };

    let error = super::resolve_active_relay_target(&settings.relay)
        .await
        .expect_err("offline relay peer must block relay stream starts");

    assert!(
        !error.trim().is_empty(),
        "expected relay probe failure to explain why the peer is unavailable"
    );
}

#[tokio::test]
async fn status_does_not_report_ready_when_selected_relay_peer_has_no_reachable_address() {
    let mut settings = settings("3840x2160", true);
    settings.relay = crate::settings::RelaySettings {
        enabled: true,
        linked_peer_id: Some("peer-1".to_string()),
        linked_peer_hostname: Some("media-box".to_string()),
        linked_peer_ip: None,
        remote_token: Some("relay-secret".to_string()),
        last_known_peer: Some(RelayPeerMetadata {
            instance_id: Some("peer-1".to_string()),
            hostname: Some("media-box".to_string()),
            ip: None,
            version: Some("1.2.3".to_string()),
            platform: Some("linux".to_string()),
            gpu_ready: Some(false),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_name: Some("RTX 4080".to_string()),
        }),
    };
    let gpu = GpuInfo {
        vendor: "intel".to_string(),
        name: Some("Intel UHD".to_string()),
        ..GpuInfo::default()
    };

    let status = build_status(&gpu, &settings, &new_session_map(), None, false);
    let status = super::apply_status_readiness(status, &gpu, &settings, false).await;

    assert_eq!(status.gpu_ready, Some(false));
}

#[test]
fn pipeline_request_only_marks_upscale_when_output_exceeds_source() {
    let request = build_pipeline_request(
        &settings("3840x2160", false),
        &source_descriptor(
            SourceTransport::RemoteHttp,
            SourceKind::Hls,
            SourceContentKind::Unknown,
            Some("1920x1080"),
            Some(30.0),
            Some(1920),
            Some(1080),
        ),
        3,
    );

    assert_eq!(request.source_transport, SourceTransport::RemoteHttp);
    assert_eq!(request.source_kind, SourceKind::Hls);
    assert_eq!(request.upscale, UpscaleRequest::Quality(3));
}

#[test]
fn pipeline_request_does_not_claim_upscale_for_matching_or_unknown_source_resolution() {
    let matching = build_pipeline_request(
        &settings("1920x1080", false),
        &source_descriptor(
            SourceTransport::RemoteHttp,
            SourceKind::Hls,
            SourceContentKind::Unknown,
            Some("1920x1080"),
            Some(30.0),
            Some(1920),
            Some(1080),
        ),
        3,
    );
    let unknown = build_pipeline_request(
        &settings("3840x2160", false),
        &SourceDescriptor {
            classification: SourceClassification {
                transport: SourceTransport::Other,
                kind: SourceKind::Other,
            },
            original_url: "input".to_string(),
            runtime_url: "input".to_string(),
            runtime_headers: HashMap::new(),
            session_headers: HashMap::new(),
            relay: None,
            metadata: None,
        },
        3,
    );

    assert_eq!(matching.upscale, UpscaleRequest::Off);
    assert_eq!(unknown.upscale, UpscaleRequest::Off);
}

#[test]
fn startup_context_keeps_source_request_capabilities_and_plan_together() {
    let settings = settings("3840x2160", true);
    let descriptor = source_descriptor(
        SourceTransport::RemoteHttp,
        SourceKind::Hls,
        SourceContentKind::Animated,
        Some("1920x1080"),
        Some(30.0),
        Some(1920),
        Some(1080),
    );
    let pipeline_request = build_pipeline_request(&settings, &descriptor, 3);
    let capabilities = planner_capabilities();

    let context = assemble_startup_context(
        runtime_backend(),
        descriptor.clone(),
        pipeline_request,
        capabilities.clone(),
        3,
    );

    assert_eq!(context.source_descriptor, descriptor);
    assert_eq!(context.planner_capabilities, capabilities);
    assert_eq!(context.pipeline_request.upscale, UpscaleRequest::Quality(3));
    assert_eq!(
        context.pipeline_request.interpolation,
        InterpolationRequest::To60
    );
    assert_eq!(context.pipeline_request.hdr, HdrRequest::Off);
    assert!(context.execution_plan.requires_local_hls_relay);
    assert_eq!(context.execution_plan.video_ops[0], VideoOp::NormalizeInput);
    assert_eq!(context.quality_level, 3);
    assert!(context.framegen_enabled);
    assert_eq!(context.target_fps, Some(60.0));
    let interpolation = context
        .execution_plan
        .interpolation_plan()
        .expect("interpolation decision");
    assert_eq!(
        interpolation.decision.realization,
        InterpolationRealization::NativeBackend
    );
}

#[test]
fn startup_context_disables_portable_framegen_for_ffmpeg_universal() {
    let settings = settings("3840x2160", true);
    let descriptor = source_descriptor(
        SourceTransport::RemoteHttp,
        SourceKind::Hls,
        SourceContentKind::Unknown,
        Some("1920x1080"),
        Some(30.0),
        Some(1920),
        Some(1080),
    );
    let capabilities = BackendCapabilities {
        executor: ExecutorKind::Universal,
        resize: ResizeSupport::Basic,
        interpolation: InterpolationSupport::Unsupported,
        hdr: HdrSupport::default(),
        upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(None),
    };
    let pipeline_request = build_pipeline_request(&settings, &descriptor, 3);

    let context = assemble_startup_context(
        runtime_backend(),
        descriptor,
        pipeline_request,
        capabilities,
        3,
    );

    assert!(!context.framegen_enabled);
    assert_eq!(context.target_fps, Some(30.0));
    let interpolation = context
        .execution_plan
        .interpolation_plan()
        .expect("interpolation decision");
    assert_eq!(
        interpolation.decision.realization,
        InterpolationRealization::Disabled
    );
    assert_eq!(
        interpolation.decision.unsupported_reason,
        Some(InterpolationUnsupportedReason::PortableFallbackNotImplemented)
    );
}

#[test]
fn startup_context_preserves_portable_framegen_for_amd_runtime_path() {
    let settings = settings("3840x2160", true);
    let descriptor = source_descriptor(
        SourceTransport::RemoteHttp,
        SourceKind::Hls,
        SourceContentKind::Unknown,
        Some("1920x1080"),
        Some(30.0),
        Some(1920),
        Some(1080),
    );
    let capabilities = BackendCapabilities {
        executor: ExecutorKind::Universal,
        resize: ResizeSupport::Basic,
        interpolation: InterpolationSupport::Unsupported,
        hdr: HdrSupport::default(),
        upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(None),
    };
    let pipeline_request = build_pipeline_request(&settings, &descriptor, 3);

    let context = assemble_startup_context(
        amd_runtime_backend(),
        descriptor,
        pipeline_request,
        capabilities,
        3,
    );

    assert!(context.framegen_enabled);
    assert_eq!(context.target_fps, Some(60.0));
    let interpolation = context
        .execution_plan
        .interpolation_plan()
        .expect("interpolation decision");
    assert_eq!(
        interpolation.decision.realization,
        InterpolationRealization::PortableFallback
    );
    assert_eq!(interpolation.decision.unsupported_reason, None);
}

// ── Approved-origins helpers ──────────────────────────────────────────────

#[test]
fn load_approved_origins_returns_empty_map_when_file_absent() {
    let path = std::env::temp_dir()
        .join(uuid::Uuid::new_v4().to_string())
        .join("approved-origins.json");
    let map = super::load_approved_origins(&path);
    assert!(map.is_empty());
}

#[test]
fn load_approved_origins_parses_valid_json_file() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("approved-origins.json");
    let json = serde_json::json!({
        "https://example.com": { "appName": "Test App", "approvedAt": "1700000000" }
    });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

    let map = super::load_approved_origins(&path);

    assert_eq!(map.len(), 1);
    let meta = map.get("https://example.com").unwrap();
    assert_eq!(meta.app_name.as_deref(), Some("Test App"));
    assert_eq!(meta.approved_at, "1700000000");
}

#[test]
fn load_approved_origins_returns_empty_map_for_corrupt_json() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("approved-origins.json");
    std::fs::write(&path, b"not-valid-json!!").unwrap();

    let map = super::load_approved_origins(&path);

    assert!(map.is_empty());
}

#[test]
fn persist_and_reload_approved_origins_survives_round_trip() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("approved-origins.json");

    let origins: DashMap<String, ApprovedOriginMeta> = DashMap::new();
    origins.insert(
        "https://docs.example.com".to_string(),
        ApprovedOriginMeta {
            app_name: Some("Docs Demo".to_string()),
            approved_at: "1700000001".to_string(),
        },
    );
    super::persist_approved_origins(&origins, &path);
    assert!(path.exists());

    let reloaded = super::load_approved_origins(&path);
    assert_eq!(reloaded.len(), 1);
    let meta = reloaded.get("https://docs.example.com").unwrap();
    assert_eq!(meta.app_name.as_deref(), Some("Docs Demo"));
    assert_eq!(meta.approved_at, "1700000001");
}

// ── handle_auth_request handler tests ────────────────────────────────────

fn make_auth_state() -> super::ServerState {
    super::ServerState {
        sessions: crate::pipeline::new_session_map(),
        instance_id: "local-instance".to_string(),
        gpu_info: Arc::new(RwLock::new(gpu_info())),
        settings: Arc::new(RwLock::new(settings("1920x1080", false))),
        encoder_capabilities: Arc::new(RwLock::new(None)),
        tmp_dir: std::env::temp_dir(),
        app_handle: None,
        api_token: Arc::new(std::sync::RwLock::new("test-secret".to_string())),
        pending_consents: Arc::new(DashMap::new()),
        approved_origins: Arc::new(DashMap::new()),
        approved_origins_path: None,
        settings_path: None,
        settings_manager: None,
        rate_limit_auth: Arc::new(RateLimiter::new(5, 60)),
        rate_limit_stream: Arc::new(RateLimiter::new(3, 60)),
        pending_consent_ui: Arc::new(std::sync::Mutex::new(None)),
        setup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    }
}

fn auth_router(state: super::ServerState) -> axum::Router {
    axum::Router::new()
        .route(
            "/v1/auth/request",
            axum::routing::post(super::handle_auth_request),
        )
        .with_state(state)
}

#[tokio::test]
async fn auth_request_missing_origin_header_returns_400() {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/auth/request")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{}"#))
        .unwrap();

    let resp = auth_router(make_auth_state()).oneshot(req).await.unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "MISSING_ORIGIN");
}

#[tokio::test]
async fn auth_request_non_http_scheme_returns_400() {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/auth/request")
        .header("Origin", "file:///local/page.html")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{}"#))
        .unwrap();

    let resp = auth_router(make_auth_state()).oneshot(req).await.unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "INVALID_ORIGIN");
}

#[tokio::test]
async fn auth_request_approved_origin_returns_token_without_consent_prompt() {
    use tower::ServiceExt;

    let approved: DashMap<String, ApprovedOriginMeta> = DashMap::new();
    approved.insert(
        "https://approved.example.com".to_string(),
        ApprovedOriginMeta {
            app_name: None,
            approved_at: "1700000000".to_string(),
        },
    );
    let state = super::ServerState {
        sessions: crate::pipeline::new_session_map(),
        instance_id: "approved-instance".to_string(),
        gpu_info: Arc::new(RwLock::new(gpu_info())),
        settings: Arc::new(RwLock::new(settings("1920x1080", false))),
        encoder_capabilities: Arc::new(RwLock::new(None)),
        tmp_dir: std::env::temp_dir(),
        app_handle: None,
        api_token: Arc::new(std::sync::RwLock::new("secret-token".to_string())),
        pending_consents: Arc::new(DashMap::new()),
        approved_origins: Arc::new(approved),
        approved_origins_path: None,
        settings_path: None,
        settings_manager: None,
        rate_limit_auth: Arc::new(RateLimiter::new(5, 60)),
        rate_limit_stream: Arc::new(RateLimiter::new(3, 60)),
        pending_consent_ui: Arc::new(std::sync::Mutex::new(None)),
        setup_complete: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    };

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/auth/request")
        .header("Origin", "https://approved.example.com")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{"appName":"Test"}"#))
        .unwrap();

    let resp = auth_router(state).oneshot(req).await.unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["token"], "secret-token");
}

#[tokio::test]
async fn handle_ping_returns_instance_identity_and_gpu_metadata() {
    let state = make_auth_state();
    let response = super::handle_ping(axum::extract::State(state)).await;
    let json = serde_json::to_value(response.0).unwrap();

    assert_eq!(json["instanceId"], "local-instance");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["gpuReady"], true);
    assert_eq!(json["gpuVendor"], "nvidia");
    assert_eq!(json["gpuName"], "NVIDIA GeForce RTX 4080");
    assert!(
        json["hostname"].as_str().is_some(),
        "expected hostname to be serialized"
    );
}

#[tokio::test]
async fn stream_settings_update_uses_shared_settings_manager() {
    let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let settings_path = dir.join("config.json");
    let settings_manager = Arc::new(SettingsManager::new(&settings_path));
    let mut state = make_auth_state();
    state.settings_manager = Some(settings_manager.clone());

    let response = super::handle_stream_settings_update(
        axum::extract::State(state),
        Json(json!({
            "resolution": "3840x2160",
            "quality": 4,
            "framegen": false,
            "hdr": true
        })),
    )
    .await;

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let updated = settings_manager.get();
    assert_eq!(updated.resolution, "3840x2160");
    assert_eq!(updated.quality, 4);
    assert!(!updated.framegen);
    assert!(updated.hdr);
}

#[cfg(not(feature = "headless"))]
#[tokio::test]
async fn auth_request_unknown_origin_without_app_handle_returns_403() {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/auth/request")
        .header("Origin", "https://unknown.example.com")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{}"#))
        .unwrap();

    let resp = auth_router(make_auth_state()).oneshot(req).await.unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "NO_APP_HANDLE");
}

#[cfg(feature = "headless")]
#[tokio::test]
async fn auth_request_unknown_origin_in_headless_mode_returns_403() {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/auth/request")
        .header("Origin", "https://unknown.example.com")
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{}"#))
        .unwrap();

    let resp = auth_router(make_auth_state()).oneshot(req).await.unwrap();

    assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "HEADLESS_MODE");
}
