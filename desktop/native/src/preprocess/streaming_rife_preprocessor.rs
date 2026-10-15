use std::io;
use std::path::{Path, PathBuf};

use crate::graph::{SharedPreprocessOperation, SharedPreprocessPlan};
use crate::runtime::{
    PipelineStageId, PipelineStageReadinessPolicy, Preprocessor, ProcessSpec, StageRuntimeContext,
};

const STREAMING_RIFE_SENTINEL: &str = "[rife_worker] ready";

/// Strips the Windows extended-length path prefix (`\\?\`) from a path string,
/// if present. Pass-through for all other paths.
///
/// The prefix is added by `std::fs::canonicalize` on Windows and prevents
/// many C-library functions (`fopen`, `LoadLibraryW`, etc.) from opening the
/// path.  The rife_worker binary uses paths as CLI arguments passed to ncnn's
/// file I/O and ffmpeg, so we strip the prefix before embedding paths.
fn strip_extended_path_prefix(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix(r"\\?\")
        .map(|t| t.to_string())
        .unwrap_or_else(|| s.into_owned())
}

/// Streaming RIFE preprocessor: drives `rife-worker` as a long-running
/// NUT-in → NUT-out filter process. Accepts HLS and non-HLS sources and any
/// source-to-target frame-rate ratio; the worker passes audio through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingRifePreprocessor {
    rife_worker_path: PathBuf,
    ffmpeg_path: PathBuf,
    model_path: PathBuf,
    pixel_format: String,
}

impl StreamingRifePreprocessor {
    /// Create a new `StreamingRifePreprocessor`.
    ///
    /// - `rife_worker_path` — path to the compiled `rife-worker` binary.
    /// - `ffmpeg_path`      — path to the bundled `ffmpeg` binary (passed through
    ///                        to rife-worker so it can decode/encode NUT).
    /// - `model_path`       — absolute path to the RIFE model directory.
    pub fn new(
        rife_worker_path: impl Into<PathBuf>,
        ffmpeg_path: impl Into<PathBuf>,
        model_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            rife_worker_path: rife_worker_path.into(),
            ffmpeg_path: ffmpeg_path.into(),
            model_path: model_path.into(),
            pixel_format: "yuv420p".to_string(),
        }
    }

    /// Output pixel format (`yuv420p` or `nv12`), to match what the executor
    /// reads best.
    pub fn with_pixel_format(mut self, pixel_format: impl Into<String>) -> Self {
        self.pixel_format = pixel_format.into();
        self
    }

    /// Finds the target FPS from the first executable interpolation operation
    /// in the shared preprocess plan.
    fn resolve_interpolation_target_fps(plan: &SharedPreprocessPlan) -> Option<u32> {
        for op_plan in &plan.operations {
            if let SharedPreprocessOperation::Interpolate(op) = &op_plan.operation {
                if op.plan.is_executable() {
                    return Some(op.plan.target_fps);
                }
            }
        }
        None
    }
}

impl Preprocessor for StreamingRifePreprocessor {
    /// Builds a `ProcessSpec` that launches `rife-worker` as a long-running
    /// NUT-in → NUT-out preprocessing stage.
    ///
    /// Returns `Err` with `io::ErrorKind::InvalidInput` if no executable
    /// interpolation plan is present, or `io::ErrorKind::Unsupported` if
    /// source FPS metadata is missing or invalid.
    fn build_preprocess_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let stage = PipelineStageId::Preprocess;
        context.transport.input.ffmpeg_input_url(stage)?;
        context.transport.output.ffmpeg_output_url(stage)?;
        let plan = context
            .execution_plan
            .to_intermediate_plan()
            .shared_preprocess_plan();

        let target_fps = Self::resolve_interpolation_target_fps(&plan).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "streaming RIFE preprocessing requires a preprocess-owned interpolation plan",
            )
        })?;

        let source_fps = context
            .source
            .metadata
            .as_ref()
            .and_then(|m| m.source_fps)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "streaming RIFE preprocessing requires source fps metadata",
                )
            })?;

        // The worker picks the GPU itself (a discrete GPU first), which on
        // hybrid laptops is the GPU the encoder uses.
        let args = vec![
            "--ffmpeg-path".to_string(),
            strip_extended_path_prefix(&self.ffmpeg_path),
            "--model-path".to_string(),
            strip_extended_path_prefix(&self.model_path),
            "--source-fps".to_string(),
            source_fps.to_string(),
            "--target-fps".to_string(),
            target_fps.to_string(),
            "--pix-fmt".to_string(),
            self.pixel_format.clone(),
        ];

        Ok(ProcessSpec::new(
            stage,
            &self.rife_worker_path,
            args,
            context.transport.clone(),
            "rife-worker",
        )
        .with_readiness_policy(PipelineStageReadinessPolicy::ReadyOnStderrSentinel {
            sentinel: STREAMING_RIFE_SENTINEL.to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::StreamingRifePreprocessor;
    use crate::graph::{
        ExecutionPlan, ExecutorKind, InterpolationDecision, InterpolationPlan, LatencyMode,
        OpExecutionStage, VideoOp,
    };
    use crate::runtime::{
        FrameTransport, Preprocessor, SessionOutputPaths, StageRuntimeContext, TransportConfig,
    };
    use crate::source::{
        SourceClassification, SourceDescriptor, SourceKind, SourceMetadata, SourceTransport,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn context(source_fps: Option<f64>) -> StageRuntimeContext {
        StageRuntimeContext {
            session_id: "session-1".to_string(),
            execution_plan: ExecutionPlan {
                executor: ExecutorKind::Universal,
                latency_mode: LatencyMode::Balanced,
                requires_local_hls_relay: false,
                video_ops: vec![
                    VideoOp::NormalizeInput,
                    VideoOp::Interpolate(InterpolationPlan {
                        target_fps: 60,
                        decision: InterpolationDecision::rife(OpExecutionStage::Preprocess),
                    }),
                ],
            },
            source: SourceDescriptor {
                classification: SourceClassification {
                    transport: SourceTransport::RemoteHttp,
                    kind: SourceKind::Other,
                },
                original_url: "https://example.com/video.mp4".to_string(),
                runtime_url: "https://example.com/video.mp4".to_string(),
                runtime_headers: HashMap::new(),
                session_headers: HashMap::new(),
                relay: None,
                metadata: source_fps.map(|fps| SourceMetadata {
                    width: None,
                    height: None,
                    source_resolution: None,
                    source_fps: Some(fps),
                    content_kind: crate::source::SourceContentKind::Unknown,
                    content_kind_confidence: None,
                }),
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session/index.m3u8"),
            },
            transport: TransportConfig {
                input: FrameTransport::StdoutPipe,
                output: FrameTransport::StdoutPipe,
            },
        }
    }

    #[test]
    fn build_preprocess_spec_emits_correct_worker_invocation() {
        let preprocessor = StreamingRifePreprocessor::new(
            "rife-worker",
            "ffmpeg",
            "models/rife-v4.25-lite_ensembleFalse",
        );
        let spec = preprocessor
            .build_preprocess_spec(&context(Some(30.0)))
            .unwrap();

        assert_eq!(spec.program, PathBuf::from("rife-worker"));
        assert!(spec
            .args
            .windows(2)
            .any(|w| w == ["--ffmpeg-path", "ffmpeg"]));
        assert!(spec
            .args
            .windows(2)
            .any(|w| w == ["--model-path", "models/rife-v4.25-lite_ensembleFalse"]));
        assert!(spec.args.windows(2).any(|w| w[0] == "--source-fps"));
        assert!(spec.args.windows(2).any(|w| w == ["--target-fps", "60"]));
        assert!(spec.args.windows(2).any(|w| w == ["--pix-fmt", "yuv420p"]));
        assert!(!spec.args.iter().any(|arg| arg == "--gpu-id"));

        let nv12 = preprocessor
            .with_pixel_format("nv12")
            .build_preprocess_spec(&context(Some(30.0)))
            .unwrap();
        assert!(nv12.args.windows(2).any(|w| w == ["--pix-fmt", "nv12"]));
    }

    #[test]
    fn build_preprocess_spec_rejects_missing_source_fps() {
        let preprocessor = StreamingRifePreprocessor::new(
            "rife-worker",
            "ffmpeg",
            "models/rife-v4.25-lite_ensembleFalse",
        );
        let err = preprocessor
            .build_preprocess_spec(&context(None))
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        assert!(err.to_string().contains("source fps"));
    }

    #[test]
    fn build_preprocess_spec_accepts_hls_sources() {
        let mut ctx = context(Some(24.0));
        ctx.source.classification.kind = SourceKind::Hls;
        let preprocessor = StreamingRifePreprocessor::new(
            "rife-worker",
            "ffmpeg",
            "models/rife-v4.25-lite_ensembleFalse",
        );
        assert!(preprocessor.build_preprocess_spec(&ctx).is_ok());
    }

    #[test]
    fn readiness_policy_is_sentinel() {
        use crate::runtime::PipelineStageReadinessPolicy;
        let preprocessor = StreamingRifePreprocessor::new(
            "rife-worker",
            "ffmpeg",
            "models/rife-v4.25-lite_ensembleFalse",
        );
        let spec = preprocessor
            .build_preprocess_spec(&context(Some(30.0)))
            .unwrap();
        assert!(matches!(
            spec.readiness_policy,
            PipelineStageReadinessPolicy::ReadyOnStderrSentinel { .. }
        ));
    }
}
