//! Session records and lifecycle: the session map, startup readiness, and
//! teardown.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::graph::ExecutorKind;
use crate::settings::RelayPeerMetadata;
use crate::source::easy_proxy::EasyProxyRoute;
use crate::source::SourceContentKind;

/// How long `cleanup_session` waits for the pipeline task to reap its stages
/// before deleting the session directory anyway.
const PIPELINE_CLEANUP_TIMEOUT_MS: u64 = 10_000;

fn pipeline_cleanup_timeout() -> std::time::Duration {
    std::time::Duration::from_millis(PIPELINE_CLEANUP_TIMEOUT_MS)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub source_url: String,
    pub output_url: String,
    pub app_name: Option<String>,
    pub stream_title: Option<String>,
    pub source_content_kind: Option<SourceContentKind>,
    pub upscaler: Option<String>,
    pub source_resolution: Option<String>,
    pub output_resolution: String,
    pub source_fps: Option<f64>,
    pub target_fps: Option<f64>,
    pub framegen_enabled: bool,
    /// Engine generating frames: "NVIDIA FRUC", "NVIDIA Framegen", or
    /// "Portable". `None` when frame generation does not run.
    #[serde(default)]
    pub framegen_engine: Option<String>,
    pub hdr_enabled: bool,
    pub quality_level: u8,
    pub executor: ExecutorKind,
    pub encoder_backend: Option<String>,
    /// Client-facing startup flag. Startup is considered complete once the
    /// packager-owned final HLS playlist exists and is non-empty.
    pub startup_complete: bool,
    pub retrying_startup: bool,
    /// Current startup stage for client polling. Values: "starting" | "ready".
    pub startup_stage: String,
}

#[derive(Debug, Clone)]
pub struct PipelineCompletionSignal {
    inner: Arc<PipelineCompletionState>,
}

#[derive(Debug)]
struct PipelineCompletionState {
    complete: AtomicBool,
    notify: tokio::sync::Notify,
}

impl PipelineCompletionSignal {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PipelineCompletionState {
                complete: AtomicBool::new(false),
                notify: tokio::sync::Notify::new(),
            }),
        }
    }

    fn is_complete(&self) -> bool {
        self.inner.complete.load(Ordering::Acquire)
    }

    pub(crate) fn mark_complete(&self) {
        if !self.inner.complete.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    async fn wait(&self) {
        loop {
            if self.is_complete() {
                return;
            }

            let notified = self.inner.notify.notified();
            if self.is_complete() {
                return;
            }
            notified.await;
        }
    }

    async fn wait_timeout(&self, timeout: std::time::Duration) -> bool {
        if self.is_complete() {
            return true;
        }

        tokio::time::timeout(timeout, self.wait()).await.is_ok()
    }
}

impl Default for PipelineCompletionSignal {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Session {
    pub info: SessionInfo,
    pub backing: SessionBacking,
}

#[derive(Debug, Clone)]
pub struct LocalSessionBacking {
    pub source_headers: HashMap<String, String>,
    /// EasyProxy server the source is routed through, captured at start so a
    /// settings change does not reroute a running stream.
    pub easy_proxy: Option<EasyProxyRoute>,
    /// Sending on this channel signals the pipeline task that the client is alive.
    /// Dropping it (via cleanup_session) signals the task to stop the running
    /// pipeline stages, including any inline or standalone packager work.
    pub heartbeat_tx: tokio::sync::mpsc::Sender<()>,
    pub session_dir: PathBuf,
    pub completion: PipelineCompletionSignal,
    pub startup_attempt: u32,
    pub startup_mode: String,
}

#[derive(Debug, Clone)]
pub struct RemoteSessionBacking {
    pub remote_base_url: String,
    pub remote_session_id: String,
    pub remote_token: String,
    #[allow(dead_code)]
    pub selected_peer: RelayPeerMetadata,
    /// Sending on this channel tells the relay keepalive task that the client is
    /// alive. Dropping it (via cleanup_session) ends the task, which stops
    /// heartbeating the peer so the peer's own inactivity timeout reaps the
    /// remote session even if the forwarded stop never arrives.
    pub heartbeat_tx: tokio::sync::mpsc::Sender<()>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SessionBacking {
    Local(LocalSessionBacking),
    Remote(RemoteSessionBacking),
}

impl Session {
    pub fn new_local(
        info: SessionInfo,
        source_headers: HashMap<String, String>,
        heartbeat_tx: tokio::sync::mpsc::Sender<()>,
        session_dir: PathBuf,
        completion: PipelineCompletionSignal,
        startup_attempt: u32,
        startup_mode: impl Into<String>,
    ) -> Self {
        Self {
            info,
            backing: SessionBacking::Local(LocalSessionBacking {
                source_headers,
                easy_proxy: None,
                heartbeat_tx,
                session_dir,
                completion,
                startup_attempt,
                startup_mode: startup_mode.into(),
            }),
        }
    }

    pub fn new_remote(
        info: SessionInfo,
        remote_base_url: String,
        remote_session_id: String,
        remote_token: String,
        selected_peer: RelayPeerMetadata,
        heartbeat_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        Self {
            info,
            backing: SessionBacking::Remote(RemoteSessionBacking {
                remote_base_url,
                remote_session_id,
                remote_token,
                selected_peer,
                heartbeat_tx,
            }),
        }
    }

    pub fn local_backing(&self) -> Option<&LocalSessionBacking> {
        match &self.backing {
            SessionBacking::Local(local) => Some(local),
            SessionBacking::Remote(_) => None,
        }
    }

    pub fn local_backing_mut(&mut self) -> Option<&mut LocalSessionBacking> {
        match &mut self.backing {
            SessionBacking::Local(local) => Some(local),
            SessionBacking::Remote(_) => None,
        }
    }

    pub fn remote_backing(&self) -> Option<&RemoteSessionBacking> {
        match &self.backing {
            SessionBacking::Remote(remote) => Some(remote),
            SessionBacking::Local(_) => None,
        }
    }

    /// Routes a local session's source through `easy_proxy`.
    pub fn with_easy_proxy(mut self, easy_proxy: Option<EasyProxyRoute>) -> Self {
        if let Some(local) = self.local_backing_mut() {
            local.easy_proxy = easy_proxy;
        }
        self
    }

    pub fn source_headers(&self) -> Option<&HashMap<String, String>> {
        self.local_backing().map(|local| &local.source_headers)
    }

    pub fn easy_proxy(&self) -> Option<&EasyProxyRoute> {
        self.local_backing()
            .and_then(|local| local.easy_proxy.as_ref())
    }

    pub fn session_dir(&self) -> Option<&Path> {
        self.local_backing()
            .map(|local| local.session_dir.as_path())
    }

    pub fn try_send_heartbeat(&self) {
        let heartbeat_tx = match &self.backing {
            SessionBacking::Local(local) => &local.heartbeat_tx,
            SessionBacking::Remote(remote) => &remote.heartbeat_tx,
        };
        let _ = heartbeat_tx.try_send(());
    }

    pub fn update_local_startup_attempt(&mut self, startup_attempt: u32, startup_mode: String) {
        if let Some(local) = self.local_backing_mut() {
            local.startup_attempt = startup_attempt;
            local.startup_mode = startup_mode;
        }
        self.info.retrying_startup = false;
    }

    pub fn set_retrying_startup(&mut self, retrying: bool) {
        self.info.retrying_startup = retrying;
    }

    pub fn mark_startup_ready(&mut self) {
        self.info.startup_complete = true;
        self.info.retrying_startup = false;
        self.info.startup_stage = "ready".to_string();
    }
}

pub type SessionMap = Arc<DashMap<String, Session>>;

pub fn new_session_map() -> SessionMap {
    Arc::new(DashMap::new())
}

/// Removes a session from the active session map and logs an info message if a session was removed.
///
/// # Examples
///
pub(super) fn remove_session_from_pipeline(session_id: &str, sessions: &SessionMap) {
    if sessions.remove(session_id).is_some() {
        debug!(
            "[Pipeline]: Removed session {} from the active session map.",
            session_id
        );
    }
}

/// Remove a session's directory if it exists.
///
/// Attempts to recursively delete `session_dir`. If the directory is already missing,
/// the function returns quietly. Any other I/O error is logged with the provided
/// `session_id` and path.
///
/// # Parameters
///
/// - `session_id`: Identifier used for log messages when removal fails.
/// - `session_dir`: Filesystem path of the session directory to remove.
///
/// # Examples
///
async fn remove_session_dir(session_id: &str, session_dir: &Path) {
    match tokio::fs::remove_dir_all(session_dir).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            warn!(
                "[Pipeline]: Failed to remove session directory for session {} at {}: {}",
                session_id,
                session_dir.display(),
                error
            );
        }
    }
}

pub(super) async fn finalize_pipeline_session(
    session_id: &str,
    sessions: &SessionMap,
    session_dir: &Path,
    completion: &PipelineCompletionSignal,
) {
    remove_session_from_pipeline(session_id, sessions);
    remove_session_dir(session_id, session_dir).await;
    completion.mark_complete();
}

/// Clean up and terminate a pipeline session.
///
/// Removes the session entry from `sessions`, signals the running pipeline to stop
/// by dropping its heartbeat sender, waits up to PIPELINE_CLEANUP_TIMEOUT_MS for
/// the pipeline supervisor to finish cleaning up spawned stages, and then removes
/// the session directory. Logs a warning if the supervisor does not finish before
/// the timeout.
///
/// # Parameters
///
/// - `session_id`: Identifier of the session to remove.
/// - `sessions`: Shared session map containing the session to clean up.
///
/// # Examples
///
pub async fn cleanup_session(session_id: &str, sessions: &SessionMap) {
    if let Some((_, session)) = sessions.remove(session_id) {
        debug!("[Pipeline]: Cleaning up session {}...", session_id);
        match session.backing {
            SessionBacking::Local(LocalSessionBacking {
                heartbeat_tx,
                session_dir,
                completion,
                ..
            }) => {
                // Dropping heartbeat_tx signals the pipeline task; completion confirms
                // the supervisor has killed and reaped all children before final cleanup.
                drop(heartbeat_tx);

                if !completion.wait_timeout(pipeline_cleanup_timeout()).await {
                    warn!(
                        "[Pipeline]: Timed out waiting for pipeline task to finish cleaning up session {} after {} ms.",
                        session_id, PIPELINE_CLEANUP_TIMEOUT_MS
                    );
                }

                remove_session_dir(session_id, &session_dir).await;
            }
            SessionBacking::Remote(_) => {
                debug!(
                    "[Pipeline]: Cleaned up remote-backed session {} without local pipeline teardown.",
                    session_id
                );
            }
        }
    }
}

pub enum PlaylistWaitOutcome {
    Ready,
    SessionEnded,
    TimedOut,
}

/// Waits for the packager-owned final HLS playlist file to exist and become
/// non-empty. This remains the session startup readiness gate even when the
/// current backend writes that playlist inline from a legacy single process.
pub async fn wait_for_packager_playlist(
    packager_playlist_path: &Path,
    session_id: &str,
    sessions: &SessionMap,
    max_wait_ms: u64,
) -> PlaylistWaitOutcome {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(max_wait_ms);

    loop {
        if let Ok(meta) = tokio::fs::metadata(packager_playlist_path).await {
            if meta.len() > 0 {
                return PlaylistWaitOutcome::Ready;
            }
        }
        if !sessions.contains_key(session_id) {
            return PlaylistWaitOutcome::SessionEnded;
        }
        if start.elapsed() > timeout {
            return PlaylistWaitOutcome::TimedOut;
        }
        // A metadata stat is cheap; poll often so stream start returns close
        // to the moment the first segment lands.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
