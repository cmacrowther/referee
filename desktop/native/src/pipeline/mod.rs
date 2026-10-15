//! Session pipeline orchestration.
//!
//! - `capabilities`: encoder capability probing and executor-kind selection,
//!   which feed graph planning.
//! - `framegen`: frame generation targets and their resolution to an output
//!   frame rate.
//! - `launch`: building the runtime executor and launching an attempt as one
//!   process or as a staged normalizer → preprocess → executor → packager
//!   graph.
//! - `run`: the per-session task that supervises attempts and retries
//!   transient startup failures.
//! - `session`: session records, the session map, readiness, and teardown.
//! - `profile`: encoder rate-control profiles.

mod capabilities;
mod framegen;
mod launch;
mod profile;
mod run;
mod session;
#[cfg(test)]
mod tests;

pub(crate) use capabilities::{
    amd_ai_available, nvidia_ai_available, select_executor_kind_for_request,
    ExecutorSelectionContext,
};
pub use capabilities::{detect_encoder_capabilities, EncoderCapabilities, ExecutorPreference};
pub use framegen::{FramegenEngine, FramegenSettings, FramegenTarget};
pub use launch::{PipelineLaunch, RuntimeBackend};
pub(crate) use launch::{HEARTBEAT_TIMEOUT_MS, ORPHAN_SESSION_TIMEOUT_MS};
pub(crate) use profile::MAX_VSR_QUALITY;
pub use profile::{clamp_quality, get_encoding_profile, EncodingProfile};
pub use run::start_pipeline;
#[allow(unused_imports)]
pub use session::{
    cleanup_session, new_session_map, wait_for_packager_playlist, LocalSessionBacking,
    PipelineCompletionSignal, PlaylistWaitOutcome, RemoteSessionBacking, Session, SessionBacking,
    SessionInfo, SessionMap,
};
