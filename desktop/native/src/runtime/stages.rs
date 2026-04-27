use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::{ChildStdin, ChildStdout, Command};

use crate::graph::{ExecutionPlan, ExecutorKind};
use crate::source::SourceDescriptor;

use super::{PipelineStageHandle, PipelineStageId, PipelineStageReadinessPolicy, StderrMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameTransport {
    SourcePull,
    StdoutPipe,
    /// Named-pipe transport. Constructed in test contexts and used in match arms;
    /// not yet wired in the live pipeline.
    #[allow(dead_code)]
    NamedPipe(PathBuf),
    /// Declared future transport for local UNIX/named socket handoff.
    /// Not yet constructed at runtime — kept in match arms for exhaustiveness.
    #[allow(dead_code)]
    LocalSocket(PathBuf),
    HlsOutput {
        playlist_path: PathBuf,
        segment_dir: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinMode {
    Null,
    Transport(FrameTransport),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdoutMode {
    Null,
    Transport(FrameTransport),
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

impl SessionOutputPaths {
    #[allow(dead_code)]
    pub fn output_dir(&self) -> &Path {
        &self.session_dir
    }
}

#[derive(Debug, Clone)]
pub struct StageRuntimeContext {
    /// Session identifier forwarded to executor log labels and future
    /// per-stage tracing. Not actively read at spawn time.
    #[allow(dead_code)]
    pub session_id: String,
    pub execution_plan: ExecutionPlan,
    pub source: SourceDescriptor,
    pub session_paths: SessionOutputPaths,
    /// Transport endpoints for this stage. Read by executor/normalizer/packager
    /// builders when constructing their commands.
    pub transport: TransportConfig,
}

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub stage: PipelineStageId,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Transport configuration for documentation and future validation.
    /// Spawn logic reads `.stdin` / `.stdout` mode fields directly.
    #[allow(dead_code)]
    pub transport: TransportConfig,
    pub stdin: StdinMode,
    pub stdout: StdoutMode,
    pub stderr_piped: bool,
    pub current_dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub readiness_policy: PipelineStageReadinessPolicy,
    pub log_label: String,
    pub stderr_mode: StderrMode,
    pub kill_on_drop: bool,
    pub hidden_window: bool,
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

impl ProcessSpec {
    /// Spawns the configured process and returns a handle together with any piped stdio.
    ///
    /// This creates a child process according to the spec's program, arguments, working
    /// directory, environment, and stdio wiring, and wraps the child in a `PipelineStageSpawn`.
    ///
    /// # Returns
    ///
    /// A `PipelineStageSpawn` containing the created `PipelineStageHandle` and optional
    /// `stdin`/`stdout` streams when those were configured as piped.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if process creation or stdio configuration fails.
    ///
    /// # Examples
    ///
    pub fn spawn_stage(&self) -> io::Result<PipelineStageSpawn> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(self.stdin.as_stdio()?)
            .stdout(self.stdout.as_stdio()?)
            .kill_on_drop(self.kill_on_drop);

        if self.stderr_piped {
            cmd.stderr(Stdio::piped());
        } else {
            cmd.stderr(Stdio::inherit());
        }

        if let Some(current_dir) = &self.current_dir {
            cmd.current_dir(current_dir);
        }

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        #[cfg(target_os = "windows")]
        if self.hidden_window {
            cmd.creation_flags(0x08000000);
        }

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

    pub fn spawn_stage_handle(&self) -> io::Result<PipelineStageHandle> {
        Ok(self.spawn_stage()?.into_handle())
    }
}

impl FrameTransport {
    fn as_stdin_stdio(&self) -> io::Result<Stdio> {
        match self {
            Self::StdoutPipe => Ok(Stdio::piped()),
            transport => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "stdin transport {:?} is declared but not yet wired by ProcessSpec::spawn_stage_handle",
                    transport
                ),
            )),
        }
    }

    fn as_stdout_stdio(&self) -> io::Result<Stdio> {
        match self {
            Self::StdoutPipe => Ok(Stdio::piped()),
            transport => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "stdout transport {:?} is declared but not yet wired by ProcessSpec::spawn_stage_handle",
                    transport
                ),
            )),
        }
    }
}

impl StdinMode {
    fn as_stdio(&self) -> io::Result<Stdio> {
        match self {
            Self::Null => Ok(Stdio::null()),
            Self::Transport(transport) => transport.as_stdin_stdio(),
        }
    }
}

impl StdoutMode {
    fn as_stdio(&self) -> io::Result<Stdio> {
        match self {
            Self::Null => Ok(Stdio::null()),
            Self::Transport(transport) => transport.as_stdout_stdio(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutorSpec {
    /// Executor kind recorded for tracing and future dispatch decisions.
    #[allow(dead_code)]
    pub kind: ExecutorKind,
    pub process: ProcessSpec,
    pub startup_label: Option<String>,
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
