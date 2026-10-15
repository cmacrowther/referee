use std::io;
use std::path::PathBuf;

use crate::exec::libplacebo_filters::LibplaceboParams;
use crate::graph::{
    HdrTransformKind, OutputBitDepth, SharedPreprocessExecutionMode, SharedPreprocessOperation,
    SharedPreprocessPlan,
};
use crate::runtime::{PipelineStageId, Preprocessor, ProcessSpec, StageRuntimeContext};

/// Filter parameters derived from `PreprocessorOwned` ops in a
/// [`SharedPreprocessPlan`].  Resize, HDR transform, and portable frame
/// interpolation are renderable here; shader upscale ops that arrive as
/// `PreprocessorOwned` are still skipped until their renderers land.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PreprocessFilterPlan {
    libplacebo: LibplaceboParams,
    /// Target frame rate for portable frame generation.  Populated when a
    /// preprocess-owned interpolation op is present and executable (i.e.
    /// `InterpolationDecision::portable_fallback` with no gap reason).
    /// `None` suppresses the interpolation filter entirely.
    target_fps: Option<u32>,
}

impl PreprocessFilterPlan {
    /// Collects the parameters of the preprocessor-owned operations in a
    /// shared preprocess plan. Shader upscale operations are skipped.
    fn from_shared_preprocess_plan(plan: &SharedPreprocessPlan) -> Self {
        let mut filter_plan = Self::default();

        for op_plan in &plan.operations {
            if op_plan.execution_mode != SharedPreprocessExecutionMode::PreprocessorOwned {
                continue;
            }
            match &op_plan.operation {
                SharedPreprocessOperation::Resize(op) => {
                    filter_plan.libplacebo.apply_resize(&op.plan);
                }
                SharedPreprocessOperation::HdrTransform(op) => {
                    filter_plan.libplacebo.apply_hdr_color(&op.hdr);
                    // NvidiaTrueHdrTonemapToHdr10 is a native NVIDIA operation
                    // and must never reach a portable preprocess stage.
                    filter_plan.libplacebo.tone_map_to_hdr10 =
                        matches!(op.plan.kind, HdrTransformKind::TonemapToHdr10);
                }
                SharedPreprocessOperation::Interpolate(op) => {
                    // is_executable() is true for portable_fallback (no gap
                    // reason) and false for portable_fallback_with_gap, so
                    // only confirmed-renderable interpolation plans populate
                    // target_fps.
                    if op.plan.is_executable() {
                        filter_plan.target_fps = Some(op.plan.target_fps);
                    }
                }
                SharedPreprocessOperation::Anime4k2xUpscale(_)
                | SharedPreprocessOperation::Artcnn2xUpscale(_) => {
                    // Shader upscale has no portable preprocess renderer yet.
                    // Skip silently rather than panic so planner changes
                    // cannot crash the process at command-build time.
                }
            }
        }

        filter_plan
    }

    /// Builds the filter graph: libplacebo (resize / HDR) first, so
    /// interpolation runs on already-scaled and color-processed frames, then
    /// `minterpolate`. Returns `None` when no filters are required.
    fn build_filter_graph(&self) -> Option<String> {
        let mut filters = Vec::new();

        if self.libplacebo.has_resize_or_color_work()
            || self.libplacebo.output_bit_depth == OutputBitDepth::Bit10
        {
            filters.push(self.libplacebo.render(false, None));
        }

        if let Some(fps) = self.target_fps {
            filters.push(format!(
                "minterpolate=fps={}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1",
                fps
            ));
        }

        if filters.is_empty() {
            None
        } else {
            Some(filters.join(","))
        }
    }
}

/// Portable preprocess stage: renders preprocessor-owned resize, HDR, and
/// interpolation on NUT frames from the normalizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegPreprocessor {
    program: PathBuf,
}

impl FfmpegPreprocessor {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// Builds the FFmpeg preprocess process. It applies a filter graph
    /// (libplacebo and/or minterpolate) and emits rawvideo when
    /// preprocessor-owned operations are renderable, and stream-copies
    /// otherwise.
    pub fn build_command(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let stage = PipelineStageId::Preprocess;
        let input_url = context.transport.input.ffmpeg_input_url(stage)?;
        let output_url = context.transport.output.ffmpeg_output_url(stage)?;
        let preprocess_plan = self.planned_shared_preprocess(context);

        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-nostdin".to_string(),
            "-fflags".to_string(),
            "+discardcorrupt+nobuffer".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            "-i".to_string(),
            input_url.to_string(),
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
        ];

        let filter_plan = PreprocessFilterPlan::from_shared_preprocess_plan(&preprocess_plan);
        // Without preprocess-owned ops, executor fallbacks and deferred
        // portable ops stay where they are and frames pass straight through.
        // Preprocess-owned ops with no renderable filter (only shader upscale)
        // are also copied until their renderers land.
        let filter_graph = preprocess_plan
            .requires_stage()
            .then(|| filter_plan.build_filter_graph())
            .flatten();

        match filter_graph {
            Some(filter) => args.extend([
                "-vf".to_string(),
                filter,
                "-c:v".to_string(),
                "rawvideo".to_string(),
                "-pix_fmt".to_string(),
                filter_plan
                    .libplacebo
                    .output_bit_depth
                    .planar_yuv_format()
                    .to_string(),
                "-c:a".to_string(),
                "copy".to_string(),
            ]),
            None => args.extend(["-c".to_string(), "copy".to_string()]),
        }

        args.extend([
            "-flush_packets".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            output_url.to_string(),
        ]);

        Ok(ProcessSpec::new(
            stage,
            &self.program,
            args,
            context.transport.clone(),
            "ffmpeg-preprocess",
        ))
    }

    fn planned_shared_preprocess(&self, context: &StageRuntimeContext) -> SharedPreprocessPlan {
        context
            .execution_plan
            .to_intermediate_plan()
            .shared_preprocess_plan()
    }
}

impl Preprocessor for FfmpegPreprocessor {
    fn build_preprocess_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        self.build_command(context)
    }
}

#[cfg(test)]
mod tests {
    use super::FfmpegPreprocessor;
    use crate::graph::{
        Artcnn2xPlan, ColorPrimariesIntent, ExecutionPlan, ExecutorKind, HdrPlan, HdrRequest,
        HdrTransformKind, HdrTransformPlan, InterpolationDecision, InterpolationPlan, LatencyMode,
        MatrixCoefficientsIntent, OpExecutionStage, OutputBitDepth, ResizePlan,
        SharedPreprocessExecutionMode, SharedPreprocessOperation, TransferCharacteristicIntent,
        VideoOp,
    };
    use crate::runtime::{
        FrameTransport, Preprocessor, SessionOutputPaths, StageRuntimeContext, TransportConfig,
    };
    use crate::source::{SourceClassification, SourceDescriptor, SourceKind, SourceTransport};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn context(
        video_ops: Vec<VideoOp>,
        input: FrameTransport,
        output: FrameTransport,
    ) -> StageRuntimeContext {
        StageRuntimeContext {
            session_id: "session-1".to_string(),
            execution_plan: ExecutionPlan {
                executor: ExecutorKind::Universal,
                latency_mode: LatencyMode::Balanced,
                requires_local_hls_relay: true,
                video_ops,
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
            transport: TransportConfig { input, output },
        }
    }

    /// Determines whether `expected` appears as a contiguous subsequence inside `args`.
    ///
    /// # Returns
    ///
    /// `true` if `args` contains `expected` in order as a consecutive slice, `false` otherwise.
    ///
    /// # Examples
    ///
    fn has_subsequence(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }

    #[test]
    fn build_command_renders_libplacebo_filter_for_preprocess_owned_resize_and_hdr() {
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Preprocess,
                }),
                VideoOp::Hdr(HdrPlan {
                    request: HdrRequest::TonemapToHdr10,
                    output_bit_depth: OutputBitDepth::Bit10,
                    color_primaries: ColorPrimariesIntent::Bt2020,
                    transfer: TransferCharacteristicIntent::Smpte2084,
                    matrix: MatrixCoefficientsIntent::Bt2020Nc,
                    metadata: None,
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::TonemapToHdr10,
                        stage: OpExecutionStage::Preprocess,
                    }),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let preprocess_plan = preprocessor.planned_shared_preprocess(&context);
        let command = preprocessor.build_command(&context).unwrap();

        assert!(preprocess_plan.requires_stage());
        assert_eq!(preprocess_plan.operations.len(), 2);
        assert!(preprocess_plan.operations.iter().all(|operation| {
            operation.execution_mode == SharedPreprocessExecutionMode::PreprocessorOwned
        }));
        assert!(has_subsequence(
            &command.args,
            &["-f", "nut", "-i", "pipe:0"]
        ));
        // A real libplacebo filter should be emitted; copy passthrough must be absent.
        let vf_pos = command.args.iter().position(|a| a == "-vf");
        assert!(vf_pos.is_some(), "expected -vf arg");
        let filter_str = &command.args[vf_pos.unwrap() + 1];
        assert!(
            filter_str.starts_with("libplacebo="),
            "expected libplacebo filter, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("w=3840"),
            "filter should contain w=3840, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("h=2160"),
            "filter should contain h=2160, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("force_original_aspect_ratio=decrease"),
            "filter should preserve source aspect ratio, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("normalize_sar=true"),
            "filter should normalize SAR with padding, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("pad_crop_ratio=0.0"),
            "filter should prefer padding over cropping, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("upscaler=spline36"),
            "quality=2 should map to spline36, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("tonemapping=bt.2390"),
            "TonemapToHdr10 should emit tonemapping=bt.2390, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("format=yuv420p10le"),
            "10-bit output should use yuv420p10le, got: {}",
            filter_str
        );
        assert!(
            !command
                .args
                .windows(2)
                .any(|w| w[0] == "-c" && w[1] == "copy"),
            "copy passthrough must not appear when libplacebo filter is active"
        );
        assert!(has_subsequence(
            &command.args,
            &[
                "-c:v",
                "rawvideo",
                "-pix_fmt",
                "yuv420p10le",
                "-c:a",
                "copy"
            ]
        ));
        assert!(has_subsequence(&command.args, &["-f", "nut", "pipe:1"]));
    }

    #[test]
    fn build_command_uses_copy_passthrough_when_no_preprocess_owned_ops() {
        // Resize at Executor stage = TemporaryExecutorFallback in the preprocess
        // plan.  The preprocess stage should be a pure NUT copy.
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "2560x1440".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let preprocess_plan = preprocessor.planned_shared_preprocess(&context);
        let command = preprocessor.build_command(&context).unwrap();

        assert!(!preprocess_plan.requires_stage());
        assert!(
            preprocess_plan.has_temporary_executor_fallbacks(),
            "resize at Executor stage should be a temporary fallback"
        );
        assert!(has_subsequence(&command.args, &["-c", "copy"]));
        assert!(
            !command.args.iter().any(|a| a == "-vf"),
            "no -vf arg expected in passthrough path"
        );
        assert!(has_subsequence(&command.args, &["-f", "nut", "pipe:1"]));
    }

    #[test]
    fn preprocess_plan_honestly_tracks_executor_fallbacks_and_unimplemented_portable_ops() {
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "2560x1440".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
                VideoOp::Hdr(HdrPlan {
                    request: HdrRequest::TonemapToHdr10,
                    output_bit_depth: OutputBitDepth::Bit10,
                    color_primaries: ColorPrimariesIntent::Bt2020,
                    transfer: TransferCharacteristicIntent::Smpte2084,
                    matrix: MatrixCoefficientsIntent::Bt2020Nc,
                    metadata: None,
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::TonemapToHdr10,
                        stage: OpExecutionStage::Executor,
                    }),
                }),
                VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                    target_resolution: "2560x1440".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback_with_gap(
                        OpExecutionStage::Preprocess,
                        crate::graph::InterpolationUnsupportedReason::PortableFallbackNotImplemented,
                    ),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let preprocess_plan = preprocessor.planned_shared_preprocess(&context);

        assert!(!preprocess_plan.requires_stage());
        assert!(preprocess_plan.has_temporary_executor_fallbacks());
        assert!(preprocess_plan.has_unimplemented_portable_operations());
        assert_eq!(preprocess_plan.operations.len(), 4);
        assert!(matches!(
            &preprocess_plan.operations[0].operation,
            SharedPreprocessOperation::Resize(_)
        ));
        assert_eq!(
            preprocess_plan.operations[0].execution_mode,
            SharedPreprocessExecutionMode::TemporaryExecutorFallback
        );
        assert!(matches!(
            &preprocess_plan.operations[1].operation,
            SharedPreprocessOperation::HdrTransform(_)
        ));
        assert!(matches!(
            &preprocess_plan.operations[2].operation,
            SharedPreprocessOperation::Artcnn2xUpscale(_)
        ));
        assert!(matches!(
            &preprocess_plan.operations[3].operation,
            SharedPreprocessOperation::Interpolate(_)
        ));
        assert_eq!(
            preprocess_plan.operations[3].execution_mode,
            SharedPreprocessExecutionMode::DeferredUntilPortableRenderer
        );
    }

    #[test]
    fn build_preprocess_spec_keeps_stage_identity_and_rejects_non_pipe_transports() {
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let spec = preprocessor
            .build_preprocess_spec(&context(
                vec![VideoOp::NormalizeInput],
                FrameTransport::StdoutPipe,
                FrameTransport::StdoutPipe,
            ))
            .unwrap();

        assert_eq!(spec.stage, crate::runtime::PipelineStageId::Preprocess);
        assert_eq!(spec.log_label, "ffmpeg-preprocess");
        assert!(preprocessor
            .build_preprocess_spec(&context(
                vec![VideoOp::NormalizeInput],
                FrameTransport::SourcePull,
                FrameTransport::StdoutPipe,
            ))
            .is_err());
    }

    #[test]
    fn build_command_emits_minterpolate_filter_for_preprocess_owned_interpolation() {
        // When interpolation is portable_fallback (no gap reason) at Preprocess
        // stage, build_command must emit a real minterpolate filter and encode
        // the output as rawvideo NUT at the correct frame rate.
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback(
                        OpExecutionStage::Preprocess,
                    ),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let command = preprocessor.build_command(&context).unwrap();

        let vf_pos = command.args.iter().position(|a| a == "-vf");
        assert!(vf_pos.is_some(), "expected -vf arg");
        let filter_str = &command.args[vf_pos.unwrap() + 1];
        assert!(
            filter_str.contains("minterpolate="),
            "expected minterpolate filter, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("fps=60"),
            "expected fps=60, got: {}",
            filter_str
        );
        assert!(
            filter_str.contains("mi_mode=mci"),
            "expected motion-compensated mode, got: {}",
            filter_str
        );
        assert!(
            !filter_str.contains("libplacebo="),
            "no libplacebo for interpolation-only: {}",
            filter_str
        );
        // Output is rawvideo yuv420p (8-bit default, no HDR plan).
        assert!(has_subsequence(
            &command.args,
            &["-c:v", "rawvideo", "-pix_fmt", "yuv420p", "-c:a", "copy"]
        ));
    }

    #[test]
    fn build_command_chains_libplacebo_before_minterpolate_for_resize_plus_interpolation() {
        // When both resize (Preprocess stage) and interpolation are present,
        // the filter graph must be: libplacebo (scale first) then minterpolate
        // (interpolate on the already-scaled frames).
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Preprocess,
                }),
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback(
                        OpExecutionStage::Preprocess,
                    ),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let command = preprocessor.build_command(&context).unwrap();

        let vf_pos = command.args.iter().position(|a| a == "-vf");
        assert!(vf_pos.is_some(), "expected -vf arg");
        let filter_str = &command.args[vf_pos.unwrap() + 1];

        let libplacebo_pos = filter_str
            .find("libplacebo=")
            .expect("libplacebo in filter");
        let minterpolate_pos = filter_str
            .find("minterpolate=")
            .expect("minterpolate in filter");
        assert!(
            libplacebo_pos < minterpolate_pos,
            "libplacebo must precede minterpolate (scale then interpolate): {}",
            filter_str
        );
        assert!(
            filter_str.contains("w=3840"),
            "resize width: {}",
            filter_str
        );
        assert!(
            filter_str.contains("h=2160"),
            "resize height: {}",
            filter_str
        );
        assert!(
            filter_str.contains("force_original_aspect_ratio=decrease"),
            "resize should preserve aspect ratio: {}",
            filter_str
        );
        assert!(
            filter_str.contains("normalize_sar=true"),
            "resize should normalize SAR: {}",
            filter_str
        );
        assert!(filter_str.contains("fps=60"), "target fps: {}", filter_str);
        // 8-bit output (no HDR plan).
        assert!(has_subsequence(
            &command.args,
            &["-c:v", "rawvideo", "-pix_fmt", "yuv420p", "-c:a", "copy"]
        ));
    }

    #[test]
    fn build_command_chains_libplacebo_hdr_then_minterpolate_for_hdr_plus_interpolation() {
        // HDR at Preprocess stage + interpolation: libplacebo renders HDR
        // (output yuv420p10le), then minterpolate runs on the 10-bit frames.
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Hdr(HdrPlan {
                    request: HdrRequest::TonemapToHdr10,
                    output_bit_depth: OutputBitDepth::Bit10,
                    color_primaries: ColorPrimariesIntent::Bt2020,
                    transfer: TransferCharacteristicIntent::Smpte2084,
                    matrix: MatrixCoefficientsIntent::Bt2020Nc,
                    metadata: None,
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::TonemapToHdr10,
                        stage: OpExecutionStage::Preprocess,
                    }),
                }),
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback(
                        OpExecutionStage::Preprocess,
                    ),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let command = preprocessor.build_command(&context).unwrap();

        let vf_pos = command.args.iter().position(|a| a == "-vf");
        assert!(vf_pos.is_some(), "expected -vf arg");
        let filter_str = &command.args[vf_pos.unwrap() + 1];

        assert!(
            filter_str.contains("libplacebo="),
            "HDR via libplacebo: {}",
            filter_str
        );
        assert!(
            filter_str.contains("tonemapping=bt.2390"),
            "tonemapping: {}",
            filter_str
        );
        assert!(
            filter_str.contains("format=yuv420p10le"),
            "10-bit from libplacebo: {}",
            filter_str
        );
        assert!(
            filter_str.contains("minterpolate="),
            "interpolation: {}",
            filter_str
        );
        assert!(
            filter_str.find("libplacebo=").unwrap() < filter_str.find("minterpolate=").unwrap(),
            "libplacebo must precede minterpolate: {}",
            filter_str
        );
        // 10-bit rawvideo output.
        assert!(has_subsequence(
            &command.args,
            &[
                "-c:v",
                "rawvideo",
                "-pix_fmt",
                "yuv420p10le",
                "-c:a",
                "copy"
            ]
        ));
    }

    #[test]
    fn build_command_still_skips_gap_reason_interpolation() {
        // portable_fallback_with_gap still has an unresolved reason and must
        // NOT produce a filter — it should be DeferredUntilPortableRenderer
        // in the preprocess plan (owner=Deferred), so requires_stage() is
        // false and the command falls through to copy passthrough.
        let preprocessor = FfmpegPreprocessor::new("ffmpeg");
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback_with_gap(
                        OpExecutionStage::Preprocess,
                        crate::graph::InterpolationUnsupportedReason::PortableFallbackNotImplemented,
                    ),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let preprocess_plan = preprocessor.planned_shared_preprocess(&context);
        let command = preprocessor.build_command(&context).unwrap();

        // Gap-reason plan routes to Deferred, not PreprocessorOwned.
        assert!(!preprocess_plan.requires_stage());
        assert!(preprocess_plan.has_unimplemented_portable_operations());
        assert!(
            !command.args.iter().any(|a| a == "-vf"),
            "no filter for gap-reason interpolation"
        );
        assert!(has_subsequence(&command.args, &["-c", "copy"]));
    }
}
