use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{info, trace, warn};

const STDERR_TAIL_LIMIT: usize = 64_000;
const STDERR_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineStageId {
    Relay,
    Normalizer,
    Preprocess,
    Executor,
    Packager,
}

impl fmt::Display for PipelineStageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Relay => "relay",
            Self::Normalizer => "normalizer",
            Self::Preprocess => "preprocess",
            Self::Executor => "executor",
            Self::Packager => "packager",
        };
        f.write_str(label)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineStageReadinessPolicy {
    ReadyOnSpawn,
    ReadyOnHeartbeat,
    /// Stage is considered ready when a specific line is observed on its stderr.
    /// The sentinel string is matched against each trimmed stderr line (contains match).
    ReadyOnStderrSentinel {
        sentinel: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStageReadiness {
    Pending,
    WaitingForHeartbeat,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStageState {
    Starting,
    Running,
    Exited,
    TimedOut,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct PipelineStageReport {
    pub stage: PipelineStageId,
    pub state: PipelineStageState,
    pub readiness: PipelineStageReadiness,
    pub exit_code: Option<i32>,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PipelineSupervisorTimeouts {
    pub inactivity_timeout: Duration,
    pub orphan_timeout: Duration,
}

/// Controls how a stage's stderr stream is read and logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StderrMode {
    /// Pass raw stderr output directly to the log, one chunk at a time.
    Raw,
    /// Line-by-line logging with selective suppression.
    ///
    /// Lines that contain any of the `suppress` substrings are not forwarded to
    /// the log but are still appended to the tail buffer so they remain available
    /// in post-mortem stage reports. Use this for known-noisy, non-actionable
    /// messages produced by sources outside our control (e.g. malformed
    /// timestamps in a source HLS stream).
    Filtered { suppress: &'static [&'static str] },
    /// Parse FFmpeg `-progress pipe:2` key=value blocks and emit a single
    /// formatted summary line per block in the same style as NVEncC progress.
    ///
    /// `gpu_vendor` should be `Some("nvidia")` when the encode backend is NVENC
    /// so that per-block GPU utilisation is queried via nvidia-smi.
    FfmpegProgress { gpu_vendor: Option<String> },
}

struct StageStderrHandle {
    buffer: Arc<Mutex<String>>,
    task: Option<JoinHandle<()>>,
}

/// Returns true when `line` looks like a key=value pair emitted by FFmpeg's
/// `-progress` output (e.g. `frame=95`, `fps=99.58`, `progress=continue`).
/// Warning/error messages start with `[` or contain whitespace before `=`.
fn is_ffmpeg_progress_field(line: &str) -> bool {
    line.split_once('=')
        .map(|(key, _)| {
            !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .unwrap_or(false)
}

/// Retains only the trailing `STDERR_TAIL_LIMIT` bytes of a stderr capture buffer.
///
/// Called after every append to keep memory bounded without losing the most recent output,
/// which is the part most useful for diagnosing failures.
fn truncate_stderr_buffer(buf: &mut String) {
    if buf.len() > STDERR_TAIL_LIMIT {
        let tail = buf[buf.len() - STDERR_TAIL_LIMIT..].to_string();
        *buf = tail;
    }
}

impl StageStderrHandle {
    /// Creates a StageStderrHandle that captures and optionally monitors a child process's stderr.
    ///
    /// If `stderr` is `Some`, this spawns a background task that reads stderr according to `mode`,
    /// appends a bounded tail of recent stderr text to the returned handle's buffer, and logs
    /// observed lines or progress summaries. If `sentinel` is provided, the background task will
    /// send `()` on the provided oneshot sender when a stderr line contains the sentinel substring,
    /// allowing external code to observe readiness.
    ///
    /// # Examples
    ///
    fn spawn(
        stage: PipelineStageId,
        label: String,
        stderr: Option<ChildStderr>,
        mode: StderrMode,
        sentinel: Option<(String, oneshot::Sender<()>)>,
    ) -> Self {
        let buffer = Arc::new(Mutex::new(String::new()));
        let task = stderr.map(|stderr| {
            let buffer = Arc::clone(&buffer);
            match mode {
                StderrMode::Raw => {
                    if let Some((sentinel_str, sentinel_tx)) = sentinel {
                        // Sentinel-aware path: read raw bytes line-by-line using
                        // read_until so that non-UTF-8 output from native libraries
                        // (e.g. ncnn/Vulkan diagnostic messages) does not cause a
                        // silent break that swallows all subsequent stderr output
                        // including the readiness sentinel and Python error messages.
                        tokio::spawn(async move {
                            let mut reader = BufReader::new(stderr);
                            let mut line_buf: Vec<u8> = Vec::new();
                            let mut sentinel_tx = Some(sentinel_tx);
                            loop {
                                line_buf.clear();
                                match reader.read_until(b'\n', &mut line_buf).await {
                                    Ok(0) => break, // EOF
                                    Ok(_) => {
                                        // Strip trailing CR/LF before conversion.
                                        let end = line_buf
                                            .iter()
                                            .rposition(|&b| b != b'\n' && b != b'\r')
                                            .map(|i| i + 1)
                                            .unwrap_or(0);
                                        let line = String::from_utf8_lossy(&line_buf[..end]);
                                        let trimmed = line.trim();
                                        if !trimmed.is_empty() {
                                            if trimmed.starts_with("frame=")
                                                || trimmed.starts_with("size=")
                                            {
                                                trace!(
                                                    "[Pipeline:{}:{}]: {}",
                                                    stage,
                                                    label,
                                                    trimmed
                                                );
                                            } else {
                                                info!(
                                                    "[Pipeline:{}:{}]: {}",
                                                    stage, label, trimmed
                                                );
                                            }
                                            let mut output = buffer.lock().unwrap();
                                            output.push_str(trimmed);
                                            output.push('\n');
                                            truncate_stderr_buffer(&mut output);
                                        }
                                        if let Some(tx) = sentinel_tx.take() {
                                            if trimmed.contains(sentinel_str.as_str()) {
                                                let _ = tx.send(());
                                            } else {
                                                sentinel_tx = Some(tx);
                                            }
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        })
                    } else {
                        // Standard chunk-based raw path.
                        let mut stderr = stderr;
                        tokio::spawn(async move {
                            let mut chunk = [0u8; 4096];
                            loop {
                                match stderr.read(&mut chunk).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        let text = String::from_utf8_lossy(&chunk[..n]);
                                        let trimmed = text.trim();
                                        if trimmed.starts_with("frame=")
                                            || trimmed.starts_with("size=")
                                        {
                                            trace!("[Pipeline:{}:{}]: {}", stage, label, trimmed);
                                        } else {
                                            info!("[Pipeline:{}:{}]: {}", stage, label, trimmed);
                                        }
                                        let mut output = buffer.lock().unwrap();
                                        output.push_str(&text);
                                        truncate_stderr_buffer(&mut output);
                                    }
                                    Err(_) => break,
                                }
                            }
                        })
                    }
                }
                StderrMode::Filtered { suppress } => {
                    // Split the sentinel tuple so the pattern string and the
                    // oneshot sender can be captured independently by the task.
                    let (sentinel_pat, mut sentinel_tx) = match sentinel {
                        Some((pat, tx)) => (Some(pat), Some(tx)),
                        None => (None, None),
                    };
                    tokio::spawn(async move {
                        let reader = BufReader::new(stderr);
                        let mut lines = reader.lines();
                        loop {
                            match lines.next_line().await {
                                Ok(Some(line)) => {
                                    let trimmed = line.trim();
                                    if !trimmed.is_empty() {
                                        let suppressed =
                                            suppress.iter().any(|pat| trimmed.contains(*pat));
                                        if !suppressed {
                                            info!("[Pipeline:{}:{}]: {}", stage, label, trimmed);
                                        }
                                        let mut output = buffer.lock().unwrap();
                                        output.push_str(trimmed);
                                        output.push('\n');
                                        truncate_stderr_buffer(&mut output);
                                    }
                                    // Filtered mode still honours readiness sentinels so it can
                                    // be used on sentinel-gated stages without special-casing.
                                    if let Some(tx) = sentinel_tx.take() {
                                        if sentinel_pat
                                            .as_deref()
                                            .is_some_and(|pat| trimmed.contains(pat))
                                        {
                                            let _ = tx.send(());
                                        } else {
                                            sentinel_tx = Some(tx);
                                        }
                                    }
                                }
                                Ok(None) | Err(_) => break,
                            }
                        }
                    })
                }
                StderrMode::FfmpegProgress { gpu_vendor } => {
                    tokio::spawn(async move {
                        let reader = BufReader::new(stderr);
                        let mut lines = reader.lines();

                        let mut frame: Option<u64> = None;
                        let mut fps: Option<f64> = None;
                        let mut total_size: Option<u64> = None;
                        // out_time_us: present in modern FFmpeg, in microseconds (unambiguous).
                        // out_time_ms: in some versions also microseconds despite the name, in
                        // others genuine milliseconds. We prefer out_time_us when available.
                        let mut out_time_us: Option<u64> = None;
                        let mut out_time_ms_raw: Option<u64> = None;

                        let mut sys = sysinfo::System::new();
                        sys.refresh_cpu_specifics(
                            sysinfo::CpuRefreshKind::nothing().with_cpu_usage(),
                        ); // establish baseline for first delta

                        loop {
                            match lines.next_line().await {
                                Ok(Some(line)) => {
                                    let trimmed = line.trim();
                                    if is_ffmpeg_progress_field(trimmed) {
                                        if let Some(val) = trimmed.strip_prefix("frame=") {
                                            if let Ok(n) = val.trim().parse::<u64>() {
                                                frame = Some(n);
                                            }
                                        } else if let Some(val) = trimmed.strip_prefix("fps=") {
                                            if let Ok(n) = val.trim().parse::<f64>() {
                                                fps = Some(n);
                                            }
                                        } else if let Some(val) =
                                            trimmed.strip_prefix("total_size=")
                                        {
                                            if let Ok(n) = val.trim().parse::<u64>() {
                                                total_size = Some(n);
                                            }
                                        } else if let Some(val) =
                                            trimmed.strip_prefix("out_time_us=")
                                        {
                                            if let Ok(n) = val.trim().parse::<u64>() {
                                                out_time_us = Some(n);
                                            }
                                        } else if let Some(val) =
                                            trimmed.strip_prefix("out_time_ms=")
                                        {
                                            if let Ok(n) = val.trim().parse::<u64>() {
                                                out_time_ms_raw = Some(n);
                                            }
                                        } else if trimmed.starts_with("progress=") {
                                            // End of a progress block — emit a formatted summary.
                                            let frame_n = frame.unwrap_or(0);
                                            let fps_n = fps.unwrap_or(0.0);

                                            // Compute kbps from output byte count and elapsed
                                            // output time.  Prefer out_time_us (unambiguous
                                            // microseconds) over out_time_ms which is
                                            // milliseconds in FFmpeg 5+ despite the old name.
                                            let kbps_n =
                                                match (total_size, out_time_us, out_time_ms_raw) {
                                                    (Some(size), Some(us), _) if us > 0 => {
                                                        // kbps = bytes * 8 * 1000 / microseconds
                                                        size * 8 * 1000 / us
                                                    }
                                                    (Some(size), None, Some(ms)) if ms > 0 => {
                                                        // Treat out_time_ms as genuine milliseconds.
                                                        // kbps = bytes * 8 / milliseconds
                                                        size * 8 / ms
                                                    }
                                                    _ => 0,
                                                };

                                            if frame_n > 0 || kbps_n > 0 {
                                                // Sample CPU since the last progress block (~1 s).
                                                sys.refresh_cpu_specifics(
                                                    sysinfo::CpuRefreshKind::nothing()
                                                        .with_cpu_usage(),
                                                );
                                                let cpu_pct = sys.global_cpu_usage().round() as u8;

                                                // Query GPU utilisation for NVIDIA backends.
                                                let gpu_pct: Option<u8> =
                                                    if gpu_vendor.as_deref() == Some("nvidia") {
                                                        tokio::task::spawn_blocking(|| {
                                                            crate::gpu::read_utilization("nvidia")
                                                        })
                                                        .await
                                                        .ok()
                                                        .flatten()
                                                    } else {
                                                        None
                                                    };

                                                let mut msg = format!(
                                                    "{} frames: {:.2} fps, {} kbps",
                                                    frame_n, fps_n, kbps_n
                                                );
                                                if let Some(gpu) = gpu_pct {
                                                    msg.push_str(&format!(", GPU {}%", gpu));
                                                }
                                                msg.push_str(&format!(", CPU {}%", cpu_pct));

                                                info!("[Pipeline:{}:{}]: {}", stage, label, msg);
                                                let mut output = buffer.lock().unwrap();
                                                output.push_str(&msg);
                                                output.push('\n');
                                                truncate_stderr_buffer(&mut output);
                                            }
                                        }
                                        // Other known fields (speed, dup_frames, drop_frames,
                                        // stream_*_q, bitrate) are intentionally skipped.
                                    } else if !trimmed.is_empty() {
                                        // Regular log line (FFmpeg warnings/errors) — log as-is.
                                        info!("[Pipeline:{}:{}]: {}", stage, label, trimmed);
                                        let mut output = buffer.lock().unwrap();
                                        output.push_str(trimmed);
                                        output.push('\n');
                                        truncate_stderr_buffer(&mut output);
                                    }
                                }
                                Ok(None) | Err(_) => break,
                            }
                        }
                    })
                }
            }
        });

        Self { buffer, task }
    }

    async fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    async fn flush(&mut self) {
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(STDERR_FLUSH_TIMEOUT, task).await;
        }
    }

    fn snapshot(&self) -> String {
        self.buffer.lock().unwrap().clone()
    }
}

pub struct PipelineChildHandle {
    child: Child,
    stderr: StageStderrHandle,
    exit_code: Option<i32>,
}

impl PipelineChildHandle {
    /// Constructs a PipelineChildHandle from a spawned `Child`, taking ownership of the child
    /// and spawning a background stderr reader according to `mode`.
    ///
    /// The optional `sentinel` is a `(sentinel_string, oneshot::Sender<()>)` pair that, when provided,
    /// causes the stderr reader to watch for lines containing `sentinel_string` and send `()` on the
    /// provided sender once the sentinel is observed; use this to drive readiness transitions externally.
    ///
    /// # Examples
    ///
    pub fn from_child(
        stage: PipelineStageId,
        mut child: Child,
        label: impl Into<String>,
        mode: StderrMode,
        sentinel: Option<(String, oneshot::Sender<()>)>,
    ) -> Self {
        let stderr = child.stderr.take();
        Self {
            child,
            stderr: StageStderrHandle::spawn(stage, label.into(), stderr, mode, sentinel),
            exit_code: None,
        }
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    fn stderr_tail(&self) -> String {
        self.stderr.snapshot()
    }

    fn try_wait(&mut self) -> std::io::Result<Option<Option<i32>>> {
        self.child.try_wait().map(|status| {
            status.map(|status| {
                let exit_code = status.code();
                self.exit_code = exit_code;
                exit_code
            })
        })
    }

    async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    async fn abort_stderr(&mut self) {
        self.stderr.abort().await;
    }

    async fn flush_stderr(&mut self) {
        self.stderr.flush().await;
    }
}

pub struct PipelineStageHandle {
    stage: PipelineStageId,
    readiness_policy: PipelineStageReadinessPolicy,
    state: PipelineStageState,
    readiness: PipelineStageReadiness,
    child: PipelineChildHandle,
    sentinel_rx: Option<oneshot::Receiver<()>>,
}

impl PipelineStageHandle {
    /// Constructs a PipelineStageHandle for a spawned child process, initializing lifecycle state and wiring a stderr-sentinel channel when the readiness policy requires it.
    ///
    /// If `readiness_policy` is `ReadyOnStderrSentinel { sentinel }`, this function creates a oneshot channel and passes the sender into the child stderr handler so that observing the sentinel text in stderr can transition the stage to ready. The returned handle starts with state `Starting` and readiness `Pending`.
    ///
    /// # Examples
    ///
    pub fn from_child(
        stage: PipelineStageId,
        child: Child,
        label: impl Into<String>,
        readiness_policy: PipelineStageReadinessPolicy,
        stderr_mode: StderrMode,
    ) -> Self {
        let (sentinel_rx, sentinel_tx) = match &readiness_policy {
            PipelineStageReadinessPolicy::ReadyOnStderrSentinel { sentinel } => {
                let (tx, rx) = oneshot::channel();
                (Some(rx), Some((sentinel.clone(), tx)))
            }
            _ => (None, None),
        };
        Self {
            stage,
            readiness_policy,
            state: PipelineStageState::Starting,
            readiness: PipelineStageReadiness::Pending,
            child: PipelineChildHandle::from_child(stage, child, label, stderr_mode, sentinel_tx),
            sentinel_rx,
        }
    }

    pub fn stage(&self) -> PipelineStageId {
        self.stage
    }

    pub fn state(&self) -> PipelineStageState {
        self.state
    }

    pub(crate) async fn stop_now(&mut self) {
        if matches!(self.state, PipelineStageState::Exited) {
            return;
        }

        self.mark_stopped();
        self.child.kill().await;
        self.child.abort_stderr().await;
    }

    /// Checks whether the stage requires a heartbeat to become ready.
    ///
    /// # Examples
    ///
    ///
    /// @returns `true` if the stage's readiness policy is `ReadyOnHeartbeat`, `false` otherwise.
    fn tracks_heartbeat(&self) -> bool {
        matches!(
            self.readiness_policy,
            PipelineStageReadinessPolicy::ReadyOnHeartbeat
        )
    }

    /// Whether this stage's readiness is gated on an external signal (a heartbeat or an stderr sentinel).
    ///
    /// # Returns
    ///
    /// `true` if the stage requires an external readiness signal (heartbeat or stderr sentinel), `false` otherwise.
    ///
    /// # Examples
    ///
    fn awaits_readiness_signal(&self) -> bool {
        matches!(
            self.readiness_policy,
            PipelineStageReadinessPolicy::ReadyOnHeartbeat
                | PipelineStageReadinessPolicy::ReadyOnStderrSentinel { .. }
        )
    }

    /// Mark the stage as started and initialize its readiness according to the configured policy.
    ///
    /// For `ReadyOnSpawn` the stage becomes `Ready`; for `ReadyOnHeartbeat` and
    /// `ReadyOnStderrSentinel` the stage becomes `WaitingForHeartbeat`.
    ///
    /// # Examples
    ///
    fn mark_started(&mut self) {
        self.state = PipelineStageState::Running;
        self.readiness = match &self.readiness_policy {
            PipelineStageReadinessPolicy::ReadyOnSpawn => PipelineStageReadiness::Ready,
            PipelineStageReadinessPolicy::ReadyOnHeartbeat
            | PipelineStageReadinessPolicy::ReadyOnStderrSentinel { .. } => {
                PipelineStageReadiness::WaitingForHeartbeat
            }
        };
    }

    /// Marks the stage as ready if its readiness policy requires a heartbeat.
    ///
    /// This is a no-op for stages that do not track heartbeat readiness.
    ///
    /// # Examples
    ///
    fn mark_heartbeat_ready(&mut self) {
        if self.tracks_heartbeat() {
            self.readiness = PipelineStageReadiness::Ready;
        }
    }

    /// Mark the stage as ready because its stderr sentinel has been observed and drop the stored sentinel receiver.
    ///
    /// This sets the stage's readiness to `PipelineStageReadiness::Ready` and clears the internal `sentinel_rx` so the oneshot cannot be used again.
    ///
    /// # Examples
    ///
    fn mark_sentinel_ready(&mut self) {
        self.readiness = PipelineStageReadiness::Ready;
        self.sentinel_rx = None;
    }

    /// Polls the stored sentinel oneshot without blocking and advances readiness.
    ///
    /// If the internal sentinel receiver has received `()`, the method sets the
    /// stage's readiness to `Ready` and returns `true`. If no sentinel is present
    /// or it has not fired yet, the method returns `false` and leaves readiness unchanged.
    ///
    /// # Examples
    ///
    fn try_advance_sentinel_ready(&mut self) -> bool {
        let Some(rx) = &mut self.sentinel_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(()) => {
                self.mark_sentinel_ready();
                true
            }
            Err(_) => false,
        }
    }

    /// Mark the stage as exited and store its process exit code.
    ///
    /// # Examples
    ///
    fn mark_exited(&mut self, exit_code: Option<i32>) {
        self.state = PipelineStageState::Exited;
        self.child.exit_code = exit_code;
    }

    fn mark_timed_out(&mut self) {
        self.state = PipelineStageState::TimedOut;
    }

    fn mark_stopped(&mut self) {
        self.state = PipelineStageState::Stopped;
    }

    fn report(&self) -> PipelineStageReport {
        PipelineStageReport {
            stage: self.stage,
            state: self.state,
            readiness: self.readiness,
            exit_code: self.child.exit_code(),
            stderr_tail: self.child.stderr_tail(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineSupervisorStopReason {
    HeartbeatChannelClosed,
    OrphanTimeout {
        waiting_on: PipelineStageId,
    },
    InactivityTimeout {
        waiting_on: PipelineStageId,
    },
    StageExited {
        stage: PipelineStageId,
        exit_code: Option<i32>,
    },
}

#[derive(Debug, Clone)]
pub struct PipelineSupervisorOutcome {
    pub stop_reason: PipelineSupervisorStopReason,
    pub first_heartbeat_seen: bool,
    pub stage_reports: Vec<PipelineStageReport>,
}

impl PipelineSupervisorOutcome {
    pub fn stage_report(&self, stage: PipelineStageId) -> Option<&PipelineStageReport> {
        self.stage_reports
            .iter()
            .find(|report| report.stage == stage)
    }
}

pub struct PipelineSupervisor {
    session_id: String,
    timeouts: PipelineSupervisorTimeouts,
    stages: Vec<PipelineStageHandle>,
    connections: Vec<PipelineStageConnection>,
}

struct PipelineStageConnection {
    upstream: PipelineStageId,
    downstream: PipelineStageId,
    task: Option<JoinHandle<()>>,
}

impl PipelineStageConnection {
    fn spawn(
        upstream: PipelineStageId,
        downstream: PipelineStageId,
        mut stdout: ChildStdout,
        mut stdin: ChildStdin,
    ) -> Self {
        let task = tokio::spawn(async move {
            match tokio::io::copy(&mut stdout, &mut stdin).await {
                Ok(bytes) => {
                    trace!(
                        "[Pipeline:{}->{}]: forwarded {} bytes before closing the bridge.",
                        upstream,
                        downstream,
                        bytes
                    );
                }
                Err(error) => {
                    warn!(
                        "[Pipeline:{}->{}]: transport bridge ended with error: {}",
                        upstream, downstream, error
                    );
                }
            }
            let _ = stdin.shutdown().await;
        });

        Self {
            upstream,
            downstream,
            task: Some(task),
        }
    }

    async fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            trace!(
                "[Pipeline:{}->{}]: aborting transport bridge task.",
                self.upstream,
                self.downstream
            );
            task.abort();
        }
    }
}

impl PipelineSupervisor {
    pub fn new(session_id: impl Into<String>, timeouts: PipelineSupervisorTimeouts) -> Self {
        Self {
            session_id: session_id.into(),
            timeouts,
            stages: Vec::new(),
            connections: Vec::new(),
        }
    }

    pub fn add_stage(&mut self, mut stage: PipelineStageHandle) {
        stage.mark_started();
        self.stages.push(stage);
    }

    pub fn add_stage_spawn(&mut self, stage: super::PipelineStageSpawn) {
        self.add_stage(stage.into_handle());
    }

    pub fn connect_pipe_stages(
        &mut self,
        upstream: &mut super::PipelineStageSpawn,
        downstream: &mut super::PipelineStageSpawn,
    ) -> std::io::Result<()> {
        let upstream_stage = upstream.stage();
        let downstream_stage = downstream.stage();
        let stdout = upstream.take_stdout().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} stage is missing a piped stdout for the stage-to-stage transport.",
                    upstream_stage
                ),
            )
        })?;
        let stdin = downstream.take_stdin().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} stage is missing a piped stdin for the stage-to-stage transport.",
                    downstream_stage
                ),
            )
        })?;

        trace!(
            "[PipelineSupervisor]: Connecting {} -> {} over process pipe.",
            upstream_stage,
            downstream_stage
        );

        self.connections.push(PipelineStageConnection::spawn(
            upstream_stage,
            downstream_stage,
            stdout,
            stdin,
        ));

        Ok(())
    }

    #[allow(dead_code)]
    pub fn add_connected_pipe_stages(
        &mut self,
        mut upstream: super::PipelineStageSpawn,
        mut downstream: super::PipelineStageSpawn,
    ) -> std::io::Result<()> {
        self.connect_pipe_stages(&mut upstream, &mut downstream)?;
        self.add_stage_spawn(upstream);
        self.add_stage_spawn(downstream);
        Ok(())
    }

    /// Runs the supervisor loop that manages pipeline stages until the pipeline stops.
    ///
    /// The supervisor monitors stage processes, heartbeat messages, and readiness signals
    /// (heartbeats or stderr sentinels). It reacts to stage exits, heartbeat channel closure,
    /// and configured timeouts by performing orderly shutdown and collecting per-stage reports.
    /// The function returns when the supervisor chooses a stop reason and performs final cleanup.
    ///
    /// # Returns
    ///
    /// The final `PipelineSupervisorOutcome` containing the stop reason, whether a heartbeat
    /// was seen, and the report for each stage.
    ///
    /// # Examples
    ///
    pub async fn run(mut self, heartbeat_rx: &mut mpsc::Receiver<()>) -> PipelineSupervisorOutcome {
        let mut first_heartbeat_seen = false;
        let mut timeout_deadline = tokio::time::Instant::now() + self.timeouts.orphan_timeout;

        loop {
            if let Some((stage_index, exit_code)) = self.poll_exited_stage() {
                let stage = self.stages[stage_index].stage();
                self.stages[stage_index].mark_exited(exit_code);
                self.kill_other_stages(stage_index).await;
                self.abort_all_connections().await;
                self.stages[stage_index].child.flush_stderr().await;

                return self.outcome(
                    PipelineSupervisorStopReason::StageExited { stage, exit_code },
                    first_heartbeat_seen,
                );
            }

            tokio::select! {
                message = heartbeat_rx.recv() => {
                    match message {
                        Some(_) => {
                            trace!("[PipelineSupervisor]: Heartbeat received for session {}", self.session_id);
                            first_heartbeat_seen = true;
                            timeout_deadline =
                                tokio::time::Instant::now() + self.timeouts.inactivity_timeout;
                            for stage in &mut self.stages {
                                stage.mark_heartbeat_ready();
                            }
                        }
                        None => {
                            info!(
                                "[PipelineSupervisor]: Session {} stopped (heartbeat channel closed).",
                                self.session_id
                            );
                            self.stop_all_stages().await;
                            return self.outcome(
                                PipelineSupervisorStopReason::HeartbeatChannelClosed,
                                first_heartbeat_seen,
                            );
                        }
                    }
                }
                _ = tokio::time::sleep_until(timeout_deadline) => {
                    let waiting_on = self
                        .stages
                        .iter()
                        .find(|stage| stage.awaits_readiness_signal())
                        .map(|stage| stage.stage())
                        .unwrap_or(PipelineStageId::Executor);

                    for stage in &mut self.stages {
                        if stage.awaits_readiness_signal() {
                            stage.mark_timed_out();
                        } else if matches!(stage.state(), PipelineStageState::Running) {
                            stage.mark_stopped();
                        }
                    }

                    self.kill_all_stage_children().await;
                    self.abort_all_connections().await;
                    self.abort_all_stage_stderr().await;

                    let reason = if first_heartbeat_seen {
                        PipelineSupervisorStopReason::InactivityTimeout { waiting_on }
                    } else {
                        PipelineSupervisorStopReason::OrphanTimeout { waiting_on }
                    };

                    return self.outcome(reason, first_heartbeat_seen);
                }
                _ = tokio::time::sleep(CHILD_POLL_INTERVAL) => {
                    // Advance any stages waiting on a stderr sentinel.
                    for stage in &mut self.stages {
                        if stage.try_advance_sentinel_ready() {
                            trace!(
                                "[PipelineSupervisor]: Stage {} sentinel ready.",
                                stage.stage()
                            );
                        }
                    }
                }
            }
        }
    }

    fn poll_exited_stage(&mut self) -> Option<(usize, Option<i32>)> {
        for (index, stage) in self.stages.iter_mut().enumerate() {
            if !matches!(stage.state(), PipelineStageState::Running) {
                continue;
            }

            match stage.child.try_wait() {
                Ok(Some(exit_code)) => return Some((index, exit_code)),
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        "[Pipeline]: I/O error polling stage {:?} exit status: {}; treating as exited.",
                        stage.stage(),
                        e
                    );
                    return Some((index, None));
                }
            }
        }
        None
    }

    async fn stop_all_stages(&mut self) {
        for stage in &mut self.stages {
            if matches!(stage.state(), PipelineStageState::Running) {
                stage.mark_stopped();
            }
        }
        self.kill_all_stage_children().await;
        self.abort_all_connections().await;
        self.abort_all_stage_stderr().await;
    }

    async fn kill_other_stages(&mut self, exited_index: usize) {
        for index in 0..self.stages.len() {
            if index == exited_index {
                continue;
            }
            if matches!(self.stages[index].state(), PipelineStageState::Running) {
                self.stages[index].mark_stopped();
                self.stages[index].child.kill().await;
                self.stages[index].child.abort_stderr().await;
            }
        }
    }

    async fn abort_all_connections(&mut self) {
        for connection in &mut self.connections {
            connection.abort().await;
        }
    }

    async fn kill_all_stage_children(&mut self) {
        for stage in &mut self.stages {
            if matches!(
                stage.state(),
                PipelineStageState::Running
                    | PipelineStageState::Stopped
                    | PipelineStageState::TimedOut
            ) {
                stage.child.kill().await;
            }
        }
    }

    async fn abort_all_stage_stderr(&mut self) {
        for stage in &mut self.stages {
            stage.child.abort_stderr().await;
        }
    }

    fn outcome(
        &self,
        stop_reason: PipelineSupervisorStopReason,
        first_heartbeat_seen: bool,
    ) -> PipelineSupervisorOutcome {
        PipelineSupervisorOutcome {
            stop_reason,
            first_heartbeat_seen,
            stage_reports: self
                .stages
                .iter()
                .map(PipelineStageHandle::report)
                .collect(),
        }
    }
}
