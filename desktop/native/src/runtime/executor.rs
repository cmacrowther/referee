use std::io;
use std::path::Path;

use crate::exec::{
    append_rigaya_denoise_args, build_rigaya_input_args, resolve_encoder_input, EncoderInput,
};
use crate::pipeline::{EncoderCapabilities, EncodingProfile};

use super::{
    BackendExecutor, ExecutorSpec, FrameTransport, PipelineStageId, PipelineStageReadinessPolicy,
    ProcessSpec, SessionOutputPaths, StageRuntimeContext, StderrMode, StdoutMode, TransportConfig,
};

#[derive(Debug, Clone)]
pub struct LegacyExecutionContext {
    pub output_resolution: String,
    pub profile: EncodingProfile,
    pub encoder_backend: String,
    pub encoder_path: std::path::PathBuf,
    pub capabilities: EncoderCapabilities,
    pub denoise: bool,
}

pub type LegacySingleProcessExecutorContext = LegacyExecutionContext;

const LEGACY_INLINE_HLS_LIST_SIZE: u32 = 8;
const LEGACY_INLINE_HLS_DELETE_SEGMENTS: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyStartupMode {
    LowLatency,
    Buffered,
}

impl LegacyStartupMode {
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

fn translate_legacy_vceenc_preset(preset: &str) -> String {
    match preset {
        "p1" | "p2" => "fast".to_string(),
        "p6" | "p7" => "slow".to_string(),
        _ => "balanced".to_string(),
    }
}

fn build_legacy_single_process_args(
    context: &LegacyExecutionContext,
    packager_playlist_path: &Path,
) -> Vec<String> {
    let mut args = Vec::new();

    args.extend([
        "--output-res".to_string(),
        context.output_resolution.clone(),
    ]);

    if context.encoder_backend == "vceenc" && context.capabilities.has_vpp_resize {
        args.push("--vpp-resize".to_string());
        args.push("amf_fsr".to_string());
    }

    args.push("--profile".to_string());
    args.push("main".to_string());

    let preset = if context.encoder_backend == "vceenc" {
        translate_legacy_vceenc_preset(&context.profile.preset)
    } else {
        context.profile.preset.clone()
    };

    args.extend([
        "-c".to_string(),
        "hevc".to_string(),
        "--preset".to_string(),
        preset,
        "--vbr".to_string(),
        context.profile.bitrate.to_string(),
        "--max-bitrate".to_string(),
        context.profile.max_bitrate.to_string(),
        "--bframes".to_string(),
        context.profile.bframes.to_string(),
    ]);

    if context.encoder_backend == "vceenc" {
        args.push("--pa".to_string());
        args.push(format!("lookahead={}", context.profile.lookahead));
    }

    // Compatibility path only: keep inline HLS muxing here until the temporary
    // single-process bridge can be deleted. The staged path should route muxing
    // through the common FFmpeg packager instead.
    args.extend([
        "--audio-codec".to_string(),
        "aac".to_string(),
        "--format".to_string(),
        "hls".to_string(),
        "--mux-option".to_string(),
        format!("hls_time:{}", context.profile.hls_time),
        "--mux-option".to_string(),
        format!("hls_list_size:{}", LEGACY_INLINE_HLS_LIST_SIZE),
        "--mux-option".to_string(),
        format!(
            "hls_flags:{}",
            if LEGACY_INLINE_HLS_DELETE_SEGMENTS {
                "delete_segments"
            } else {
                ""
            }
        ),
        "-o".to_string(),
        packager_playlist_path.to_string_lossy().to_string(),
    ]);

    args
}

/// Transitional adapter that keeps the current monolithic encoder flow alive
/// behind the new runtime executor seam. It intentionally keeps execution and
/// HLS muxing fused inside one legacy process until relay/normalizer/packager
/// are wired as independent processes, even though the final playlist is still
/// modeled as the packager-owned output of the session. Keep new backend-
/// specific processing and staged-pipeline behavior out of this bridge.
pub struct LegacySingleProcessExecutor {
    context: LegacySingleProcessExecutorContext,
}

impl LegacySingleProcessExecutor {
    pub fn new(context: LegacySingleProcessExecutorContext) -> Self {
        Self { context }
    }

    pub fn build_stage_context(
        session_id: &str,
        execution_plan: &crate::graph::ExecutionPlan,
        source: &crate::source::SourceDescriptor,
        packager_playlist_path: &Path,
    ) -> StageRuntimeContext {
        let session_dir = packager_playlist_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        StageRuntimeContext {
            session_id: session_id.to_string(),
            execution_plan: execution_plan.clone(),
            source: source.clone(),
            session_paths: SessionOutputPaths {
                session_dir: session_dir.clone(),
                packager_playlist_path: packager_playlist_path.to_path_buf(),
            },
            transport: TransportConfig {
                input: FrameTransport::SourcePull,
                output: FrameTransport::HlsOutput {
                    playlist_path: packager_playlist_path.to_path_buf(),
                    segment_dir: session_dir,
                },
            },
        }
    }
}

impl BackendExecutor for LegacySingleProcessExecutor {
    fn build_executor_spec(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> io::Result<ExecutorSpec> {
        let startup_mode = LegacyStartupMode::for_attempt(attempt_number);
        let (encoder_input, stdin) = resolve_encoder_input(context)?;
        let mut args =
            build_rigaya_input_args(&encoder_input, startup_mode.uses_low_latency_input());
        if self.context.denoise && matches!(&encoder_input, EncoderInput::SourceUrl { .. }) {
            append_rigaya_denoise_args(&mut args);
        }
        args.extend(build_legacy_single_process_args(
            &self.context,
            &context.session_paths.packager_playlist_path,
        ));

        Ok(ExecutorSpec {
            kind: context.execution_plan.executor,
            startup_label: Some(startup_mode.as_str().to_string()),
            process: ProcessSpec {
                stage: PipelineStageId::Executor,
                program: self.context.encoder_path.clone(),
                args,
                transport: context.transport.clone(),
                stdin,
                stdout: StdoutMode::Null,
                stderr_piped: true,
                current_dir: None,
                env: Vec::new(),
                readiness_policy: PipelineStageReadinessPolicy::ReadyOnHeartbeat,
                log_label: self.context.encoder_backend.clone(),
                stderr_mode: StderrMode::Raw,
                kill_on_drop: true,
                hidden_window: true,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendExecutor, LegacySingleProcessExecutor, LegacySingleProcessExecutorContext};
    use crate::graph::{ExecutionPlan, ExecutorKind, LatencyMode, VideoOp};
    use crate::pipeline::{EncoderCapabilities, EncodingProfile};
    use crate::runtime::{
        FrameTransport, SessionOutputPaths, StageRuntimeContext, StdinMode, TransportConfig,
    };
    use crate::source::{SourceClassification, SourceDescriptor, SourceKind, SourceTransport};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Creates a test LegacySingleProcessExecutor populated with typical encoder settings.
    ///
    /// # Examples
    ///
    fn test_executor() -> LegacySingleProcessExecutor {
        LegacySingleProcessExecutor::new(LegacySingleProcessExecutorContext {
            output_resolution: "1920x1080".to_string(),
            profile: EncodingProfile {
                bitrate: 25_000,
                max_bitrate: 37_500,
                preset: "p4".to_string(),
                lookahead: 8,
                bframes: 3,
                hls_time: 1,
            },
            encoder_backend: "vceenc".to_string(),
            encoder_path: PathBuf::from("VCEEncC64.exe"),
            capabilities: EncoderCapabilities {
                has_vpp_resize: true,
                has_fruc: false,
                has_truehdr: false,
                has_rife: false,
            },
            denoise: false,
        })
    }

    fn stage_context(input: FrameTransport) -> StageRuntimeContext {
        StageRuntimeContext {
            session_id: "session-1".to_string(),
            execution_plan: ExecutionPlan {
                executor: ExecutorKind::Universal,
                latency_mode: LatencyMode::Low,
                requires_local_hls_relay: true,
                video_ops: vec![VideoOp::NormalizeInput],
            },
            source: SourceDescriptor {
                classification: SourceClassification {
                    transport: SourceTransport::RemoteHttp,
                    kind: SourceKind::Hls,
                },
                original_url: "https://example.com/live/master.m3u8".to_string(),
                runtime_url: "http://127.0.0.1:14002/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8".to_string(),
                runtime_headers: HashMap::new(),
                session_headers: HashMap::new(),
                relay: None,
                metadata: None,
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session\\index.m3u8"),
            },
            transport: TransportConfig {
                input,
                output: FrameTransport::HlsOutput {
                    playlist_path: PathBuf::from("session\\index.m3u8"),
                    segment_dir: PathBuf::from("session"),
                },
            },
        }
    }

    #[test]
    fn executor_spec_keeps_source_pull_as_non_transport_stdin() {
        let context = stage_context(FrameTransport::SourcePull);
        let executor = test_executor();
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.transport.input, FrameTransport::SourcePull);
        assert_eq!(spec.process.stdin, StdinMode::Null);
    }

    #[test]
    fn executor_spec_maps_stdout_pipe_transport_to_transport_stdin() {
        let context = stage_context(FrameTransport::StdoutPipe);
        let executor = test_executor();
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.transport.input, FrameTransport::StdoutPipe);
        assert_eq!(
            spec.process.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert!(spec
            .process
            .args
            .windows(3)
            .any(|window| window == ["--avsw", "-i", "-"]));
    }

    #[test]
    fn executor_spec_uses_named_pipe_path_as_explicit_input_transport() {
        let named_pipe =
            FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1"));
        let context = stage_context(named_pipe.clone());
        let executor = test_executor();
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.transport.input, named_pipe);
        assert_eq!(spec.process.stdin, StdinMode::Null);
        assert!(spec.process.args.windows(3).any(|window| {
            window == ["--avsw", "-i", r"\\.\pipe\referee-normalized-session-1"]
        }));
    }

    #[test]
    fn legacy_single_process_bridge_keeps_amd_compatibility_flags_local_to_runtime() {
        let context = stage_context(FrameTransport::SourcePull);
        let executor = test_executor();
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--output-res", "1920x1080"]));
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--vpp-resize", "amf_fsr"]));
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--preset", "balanced"]));
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--pa", "lookahead=8"]));
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--format", "hls"]));
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| { window == ["--mux-option", "hls_list_size:8"] }));
    }
}
