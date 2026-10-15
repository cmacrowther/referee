//! Stage contracts: the context each stage builder receives
//! ([`StageRuntimeContext`]) and the child process it returns ([`ProcessSpec`]).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{ChildStdin, ChildStdout, Command};

use crate::graph::ExecutionPlan;
use crate::source::SourceDescriptor;

use super::{PipelineStageHandle, PipelineStageId, PipelineStageReadinessPolicy, StderrMode};

/// Number of segments every HLS muxer keeps in the session playlist.
pub const HLS_PLAYLIST_SIZE: u32 = 8;
/// Segment file name pattern, relative to the session directory.
pub const HLS_SEGMENT_FILENAME_PATTERN: &str = "segment_%06d.ts";

/// How a stage receives or emits frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameTransport {
    /// The stage pulls the source URL itself.
    SourcePull,
    /// Anonymous process pipe. The supervisor bridges the upstream stage's
    /// stdout into the downstream stage's stdin.
    StdoutPipe,
    /// Final HLS output: the session playlist and its segments.
    HlsOutput {
        playlist_path: PathBuf,
        segment_dir: PathBuf,
    },
}

impl FrameTransport {
    /// The error a stage builder returns for a transport it cannot handle.
    pub fn unsupported(&self, stage: PipelineStageId, direction: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "The {} stage cannot use {:?} as its {} transport.",
                stage, self, direction
            ),
        )
    }

    /// FFmpeg URL for reading this transport.
    pub fn ffmpeg_input_url(&self, stage: PipelineStageId) -> io::Result<&'static str> {
        match self {
            Self::StdoutPipe => Ok("pipe:0"),
            other => Err(other.unsupported(stage, "input")),
        }
    }

    /// FFmpeg URL for writing to this transport.
    pub fn ffmpeg_output_url(&self, stage: PipelineStageId) -> io::Result<&'static str> {
        match self {
            Self::StdoutPipe => Ok("pipe:1"),
            other => Err(other.unsupported(stage, "output")),
        }
    }

    fn stdio(&self) -> Stdio {
        match self {
            Self::StdoutPipe => Stdio::piped(),
            Self::SourcePull | Self::HlsOutput { .. } => Stdio::null(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    pub input: FrameTransport,
    pub output: FrameTransport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOutputPaths {
    pub session_dir: PathBuf,
    pub packager_playlist_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StageRuntimeContext {
    pub session_id: String,
    pub execution_plan: ExecutionPlan,
    pub source: SourceDescriptor,
    pub session_paths: SessionOutputPaths,
    /// Transport endpoints for this stage.
    pub transport: TransportConfig,
}

impl StageRuntimeContext {
    /// Context for a single-process launch: the executor pulls the source and
    /// muxes HLS straight into the session directory.
    pub fn single_process(
        session_id: &str,
        execution_plan: &ExecutionPlan,
        source: &SourceDescriptor,
        playlist_path: &Path,
    ) -> Self {
        let session_dir = playlist_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        Self {
            session_id: session_id.to_string(),
            execution_plan: execution_plan.clone(),
            source: source.clone(),
            session_paths: SessionOutputPaths {
                session_dir: session_dir.clone(),
                packager_playlist_path: playlist_path.to_path_buf(),
            },
            transport: TransportConfig {
                input: FrameTransport::SourcePull,
                output: FrameTransport::HlsOutput {
                    playlist_path: playlist_path.to_path_buf(),
                    segment_dir: session_dir,
                },
            },
        }
    }

    /// A copy of this context with different stage transports.
    pub fn with_transport(&self, input: FrameTransport, output: FrameTransport) -> Self {
        Self {
            transport: TransportConfig { input, output },
            ..self.clone()
        }
    }

    pub fn source_fps(&self) -> Option<f64> {
        self.source
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.source_fps)
    }

    /// Frame rate the executor emits, or `None` when the source frame rate is
    /// unknown.
    pub fn planned_output_fps(&self) -> Option<f64> {
        self.execution_plan.output_frame_rate(self.source_fps())
    }
}

/// Source-ingest profile for a launch attempt. The first attempt favors
/// latency; retries use larger probe and analyze windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupMode {
    LowLatency,
    Buffered,
}

impl StartupMode {
    pub fn for_attempt(attempt_number: u32) -> Self {
        if attempt_number == 1 {
            Self::LowLatency
        } else {
            Self::Buffered
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LowLatency => "low-latency",
            Self::Buffered => "buffered",
        }
    }

    pub(crate) fn uses_low_latency_input(&self) -> bool {
        matches!(self, Self::LowLatency)
    }
}

/// A stage's child process as built by its stage builder.
///
/// Every stage is spawned with piped stderr (tailed by the supervisor),
/// `kill_on_drop`, and a hidden console window on Windows. Stdin and stdout
/// are piped exactly when the matching transport is
/// [`FrameTransport::StdoutPipe`].
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub stage: PipelineStageId,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub transport: TransportConfig,
    pub current_dir: Option<PathBuf>,
    pub readiness_policy: PipelineStageReadinessPolicy,
    pub log_label: String,
    pub stderr_mode: StderrMode,
}

impl ProcessSpec {
    /// A spec with the stage's default readiness policy and raw stderr logging.
    pub fn new(
        stage: PipelineStageId,
        program: impl Into<PathBuf>,
        args: Vec<String>,
        transport: TransportConfig,
        log_label: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            program: program.into(),
            args,
            transport,
            current_dir: None,
            readiness_policy: stage.default_readiness_policy(),
            log_label: log_label.into(),
            stderr_mode: StderrMode::Raw,
        }
    }

    pub fn with_readiness_policy(mut self, readiness_policy: PipelineStageReadinessPolicy) -> Self {
        self.readiness_policy = readiness_policy;
        self
    }

    pub fn with_stderr_mode(mut self, stderr_mode: StderrMode) -> Self {
        self.stderr_mode = stderr_mode;
        self
    }

    pub fn with_current_dir(mut self, current_dir: Option<PathBuf>) -> Self {
        self.current_dir = current_dir;
        self
    }

    /// The full command line, for logging.
    pub fn command_line(&self) -> String {
        format!("{} {}", self.program.display(), self.args.join(" "))
    }

    /// Spawns the process and returns its handle together with any piped
    /// stdin/stdout, which the supervisor bridges between stages.
    pub fn spawn_stage(&self) -> io::Result<PipelineStageSpawn> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(self.transport.input.stdio())
            .stdout(self.transport.output.stdio())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(current_dir) = &self.current_dir {
            cmd.current_dir(current_dir);
        }
        crate::process::hide_tokio_command_window(&mut cmd);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();

        Ok(PipelineStageSpawn {
            handle: PipelineStageHandle::from_child(
                self.stage,
                child,
                self.log_label.clone(),
                self.readiness_policy.clone(),
                self.stderr_mode.clone(),
            ),
            stdin,
            stdout,
        })
    }
}

pub struct PipelineStageSpawn {
    handle: PipelineStageHandle,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
}

impl PipelineStageSpawn {
    pub fn stage(&self) -> PipelineStageId {
        self.handle.stage()
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    pub fn into_handle(self) -> PipelineStageHandle {
        self.handle
    }
}

#[derive(Debug, Clone)]
pub struct ExecutorSpec {
    pub process: ProcessSpec,
    pub startup_mode: StartupMode,
}

pub trait BackendExecutor: Send + Sync {
    fn build_executor_spec(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> io::Result<ExecutorSpec>;
}

pub trait Normalizer: Send + Sync {
    fn build_normalizer_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec>;
}

pub trait Preprocessor: Send + Sync {
    fn build_preprocess_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec>;
}

pub trait Packager: Send + Sync {
    fn build_packager_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec>;
}
