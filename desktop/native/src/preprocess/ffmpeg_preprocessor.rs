use std::io;
use std::path::PathBuf;

use crate::exec::libplacebo_filters::{
    map_color_primaries, map_matrix, map_quality_to_libplacebo_scaler, map_transfer,
    parse_resolution, push_aspect_preserving_resize_args,
};
use crate::graph::{
    ColorPrimariesIntent, HdrTransformKind, MatrixCoefficientsIntent, OutputBitDepth,
    SharedPreprocessExecutionMode, SharedPreprocessOperation, SharedPreprocessPlan,
    TransferCharacteristicIntent,
};
use crate::runtime::{
    FrameTransport, PipelineStageId, PipelineStageReadinessPolicy, Preprocessor, ProcessSpec,
    StageRuntimeContext, StderrMode, StdinMode, StdoutMode,
};

/// Filter parameters derived from `PreprocessorOwned` ops in a
/// [`SharedPreprocessPlan`].  Resize, HDR transform, and portable frame
/// interpolation are renderable here; shader upscale ops that arrive as
/// `PreprocessorOwned` are still skipped until their renderers land.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreprocessFilterPlan {
    output_resolution: Option<String>,
    resize_quality: Option<u8>,
    output_bit_depth: OutputBitDepth,
    color_primaries: ColorPrimariesIntent,
    transfer: TransferCharacteristicIntent,
    matrix: MatrixCoefficientsIntent,
    tone_map_to_hdr10: bool,
    /// Target frame rate for portable frame generation.  Populated when a
    /// preprocess-owned interpolation op is present and executable (i.e.
    /// `InterpolationDecision::portable_fallback` with no gap reason).
    /// `None` suppresses the interpolation filter entirely.
    target_fps: Option<u32>,
}

impl Default for PreprocessFilterPlan {
    /// Create a `PreprocessFilterPlan` with no-op defaults.
    ///
    /// The produced plan represents "do nothing" preprocessing:
    /// - no resize, no interpolation
    /// - 8-bit output bit depth
    /// - color primaries, transfer, and matrix set to preserve source
    /// - `tone_map_to_hdr10` is `false`
    ///
    /// # Examples
    ///
    fn default() -> Self {
        Self {
            output_resolution: None,
            resize_quality: None,
            output_bit_depth: OutputBitDepth::Bit8,
            color_primaries: ColorPrimariesIntent::PreserveSource,
            transfer: TransferCharacteristicIntent::PreserveSource,
            matrix: MatrixCoefficientsIntent::PreserveSource,
            tone_map_to_hdr10: false,
            target_fps: None,
        }
    }
}

impl PreprocessFilterPlan {
    /// Create a PreprocessFilterPlan by extracting and consolidating only the operations
    /// that are marked for preprocessor-owned execution from a SharedPreprocessPlan.
    ///
    /// The returned plan copies relevant parameters from preprocess-owned operations:
    /// - `Resize` sets `output_resolution` and `resize_quality`.
    /// - `HdrTransform` sets `output_bit_depth`, `color_primaries`, `transfer`, `matrix`,
    ///   and sets `tone_map_to_hdr10` when the operation kind is `TonemapToHdr10`.
    /// - `Interpolate` sets `target_fps` only when the interpolation plan is executable.
    /// Shader-based upscale operations are ignored (skipped silently).
    ///
    /// # Parameters
    ///
    /// - `plan`: the shared preprocess plan to derive from; only operations with
    ///   `execution_mode == SharedPreprocessExecutionMode::PreprocessorOwned` are considered.
    ///
    /// # Returns
    ///
    /// A `PreprocessFilterPlan` with fields populated from the matching preprocess-owned
    /// operations (or defaults when none apply).
    ///
    /// # Examples
    ///
    fn from_shared_preprocess_plan(plan: &SharedPreprocessPlan) -> Self {
        let mut filter_plan = Self::default();

        for op_plan in &plan.operations {
            if op_plan.execution_mode != SharedPreprocessExecutionMode::PreprocessorOwned {
                continue;
            }
            match &op_plan.operation {
                SharedPreprocessOperation::Resize(op) => {
                    filter_plan.output_resolution = Some(op.plan.target_resolution.clone());
                    filter_plan.resize_quality = op.plan.quality;
                }
                SharedPreprocessOperation::HdrTransform(op) => {
                    filter_plan.output_bit_depth = op.hdr.output_bit_depth;
                    filter_plan.color_primaries = op.hdr.color_primaries;
                    filter_plan.transfer = op.hdr.transfer;
                    filter_plan.matrix = op.hdr.matrix;
                    // NvidiaTrueHdrTonemapToHdr10 is a native NVIDIA operation
                    // and must never reach a portable preprocess stage.
                    filter_plan.tone_map_to_hdr10 =
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

    /// Returns whether any processing that must be performed by libplacebo is required.
    ///
    /// This is true when the plan requests a resize, a tonemap-to-HDR10, a 10-bit output
    /// bit depth, or any explicit color primaries / transfer / matrix conversion
    /// (i.e., any of those intents are not `PreserveSource`).
    ///
    /// # Examples
    ///
    fn has_libplacebo_processing(&self) -> bool {
        self.output_resolution.is_some()
            || self.tone_map_to_hdr10
            || self.output_bit_depth == OutputBitDepth::Bit10
            || !matches!(self.color_primaries, ColorPrimariesIntent::PreserveSource)
            || !matches!(self.transfer, TransferCharacteristicIntent::PreserveSource)
            || !matches!(self.matrix, MatrixCoefficientsIntent::PreserveSource)
    }

    /// Builds a `libplacebo=...` FFmpeg filter string for preprocess-owned resize and HDR operations.
    ///
    /// Returns `None` when no libplacebo-related processing is required (prevents inserting a no-op filter).
    ///
    /// # Examples
    ///
    fn build_libplacebo_filter(&self) -> Option<String> {
        if !self.has_libplacebo_processing() {
            return None;
        }

        let mut args = Vec::new();

        if let Some((width, height)) = self.output_resolution.as_deref().and_then(parse_resolution)
        {
            push_aspect_preserving_resize_args(
                &mut args,
                width,
                height,
                map_quality_to_libplacebo_scaler(self.resize_quality),
            );
        }

        if self.tone_map_to_hdr10 {
            args.push("tonemapping=bt.2390".to_string());
        }
        if let Some(primaries) = map_color_primaries(self.color_primaries) {
            args.push(format!("color_primaries={}", primaries));
        }
        if let Some(trc) = map_transfer(self.transfer) {
            args.push(format!("color_trc={}", trc));
        }
        if let Some(colorspace) = map_matrix(self.matrix) {
            args.push(format!("colorspace={}", colorspace));
        }
        args.push(format!(
            "format={}",
            if self.output_bit_depth == OutputBitDepth::Bit10 {
                "yuv420p10le"
            } else {
                "yuv420p"
            }
        ));

        Some(format!("libplacebo={}", args.join(":")))
    }

    /// Constructs a `minterpolate` FFmpeg filter configured for the plan's target frame rate.
    ///
    /// Returns `None` when no target frame rate is configured (`target_fps` is `None`).
    ///
    /// # Examples
    ///
    fn build_interpolation_filter(&self) -> Option<String> {
        let fps = self.target_fps?;
        Some(format!(
            "minterpolate=fps={}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1",
            fps
        ))
    }

    /// Builds the FFmpeg filter graph string composed of libplacebo (resize / HDR)
    /// and interpolation filters.
    ///
    /// The libplacebo filter, when present, is placed before the interpolation
    /// filter so interpolation operates on already-scaled and color-processed
    /// frames. Returns `None` when no filters are required.
    ///
    /// # Examples
    ///
    fn build_filter_graph(&self) -> Option<String> {
        let mut filters = Vec::new();

        if let Some(libplacebo) = self.build_libplacebo_filter() {
            filters.push(libplacebo);
        }

        if let Some(interpolation) = self.build_interpolation_filter() {
            filters.push(interpolation);
        }

        if filters.is_empty() {
            None
        } else {
            Some(filters.join(","))
        }
    }

    /// Selects the FFmpeg pixel format string to use for rawvideo output based on the planned output bit depth.
    ///
    /// When the plan targets 10-bit output, returns `"yuv420p10le"`; otherwise returns `"yuv420p"`.
    ///
    /// # Examples
    ///
    fn output_pixel_format(&self) -> &'static str {
        if self.output_bit_depth == OutputBitDepth::Bit10 {
            "yuv420p10le"
        } else {
            "yuv420p"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegPreprocessorCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: StdinMode,
    pub stdout: StdoutMode,
    pub output_transport: FrameTransport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegPreprocessor {
    program: PathBuf,
    output_transport: FrameTransport,
}

impl FfmpegPreprocessor {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            output_transport: FrameTransport::StdoutPipe,
        }
    }

    pub fn with_output_transport(mut self, output_transport: FrameTransport) -> Self {
        self.output_transport = output_transport;
        self
    }

    /// Builds the FFmpeg command used for the preprocess pipeline stage.

    ///

    /// This validates the provided runtime context, derives the shared preprocess

    /// plan, and constructs an `FfmpegPreprocessorCommand` that either:

    /// - applies a filter graph (libplacebo and/or minterpolate) and emits rawvideo

    ///   with an appropriate pixel format when preprocess-owned operations are

    ///   renderable, or

    /// - uses stream passthrough (`-c copy`) when no renderable preprocess filter is

    ///   produced or when the preprocess stage is not required.

    ///

    /// The returned command includes program path, FFmpeg arguments, configured

    /// stdin/stdout modes, and the associated output transport.

    ///

    /// # Examples

    ///

    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
    ) -> io::Result<FfmpegPreprocessorCommand> {
        self.validate_context(context)?;
        let preprocess_plan = self.planned_shared_preprocess(context);

        let (input_arg, stdin) = self.resolve_input_transport(&context.transport.input)?;
        let output_arg = self.output_sink()?;
        let stdout = match &self.output_transport {
            FrameTransport::StdoutPipe => StdoutMode::Transport(FrameTransport::StdoutPipe),
            FrameTransport::NamedPipe(_) => StdoutMode::Null,
            FrameTransport::LocalSocket(path) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!(
                        "FFmpeg preprocess does not yet support local-socket output at {:?}.",
                        path
                    ),
                ))
            }
            transport => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "FFmpeg preprocess cannot map {:?} to a process stdout mode.",
                        transport
                    ),
                ))
            }
        };

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
            input_arg,
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
        ];

        let filter_plan = PreprocessFilterPlan::from_shared_preprocess_plan(&preprocess_plan);

        if preprocess_plan.requires_stage() {
            // Render preprocess-owned operations.  libplacebo handles resize
            // and HDR transforms; minterpolate handles portable frame-rate
            // doubling.  Either or both may be present; build_filter_graph
            // combines them (libplacebo first, then minterpolate).
            // Shader upscale ops remain skipped until their renderers land.
            if let Some(filter) = filter_plan.build_filter_graph() {
                args.push("-vf".to_string());
                args.push(filter);
                args.extend([
                    "-c:v".to_string(),
                    "rawvideo".to_string(),
                    "-pix_fmt".to_string(),
                    filter_plan.output_pixel_format().to_string(),
                    "-c:a".to_string(),
                    "copy".to_string(),
                ]);
            } else {
                // PreprocessorOwned ops were present but none produced a
                // renderable filter (e.g. only shader upscale landed here).
                // Fall back to copy until their renderers land.
                args.extend(["-c".to_string(), "copy".to_string()]);
            }
        } else {
            // No preprocess-owned ops: passthrough copy.
            // Resize/HDR with TemporaryExecutorFallback mode stay in the
            // executor.  Deferred/unimplemented portable ops stay deferred.
            args.extend(["-c".to_string(), "copy".to_string()]);
        }

        args.extend([
            "-flush_packets".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            output_arg,
        ]);

        Ok(FfmpegPreprocessorCommand {
            program: self.program.clone(),
            args,
            stdin,
            stdout,
            output_transport: self.output_transport.clone(),
        })
    }

    /// Extracts the shared preprocess plan derived from the stage's execution plan.
    ///
    /// This converts the context's execution plan into an intermediate plan and returns
    /// its shared preprocess plan used to determine preprocess-stage operations.
    ///
    /// # Examples
    ///
    fn planned_shared_preprocess(&self, context: &StageRuntimeContext) -> SharedPreprocessPlan {
        context
            .execution_plan
            .to_intermediate_plan()
            .shared_preprocess_plan()
    }

    /// Validates that the runtime context uses an explicit inter-stage input transport and that its output transport matches this preprocessor's configured output transport.
    ///
    /// On success, the context is suitable for running the FFmpeg preprocess stage. On failure, an `io::Error` with `InvalidInput` is returned:
    /// - when `context.transport.input` is not `FrameTransport::StdoutPipe` or `FrameTransport::NamedPipe(_)`;
    /// - when `context.transport.output` does not equal the preprocessor's `output_transport`.
    ///
    /// # Parameters
    ///
    /// - `context`: runtime stage context whose input/output transports are validated.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the context is valid for the FFmpeg preprocess stage, otherwise an `Err(io::Error)` describing the invalid transport.
    ///
    /// # Examples
    ///
    fn validate_context(&self, context: &StageRuntimeContext) -> io::Result<()> {
        match context.transport.input {
            FrameTransport::StdoutPipe | FrameTransport::NamedPipe(_) => {}
            ref transport => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                    "FFmpeg preprocess requires an explicit inter-stage input transport, got {:?}.",
                    transport
                ),
                ))
            }
        }

        let expected_output = self.output_transport.clone();
        if context.transport.output != expected_output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FFmpeg preprocess requires {:?} output transport, got {:?}.",
                    expected_output, context.transport.output
                ),
            ));
        }

        Ok(())
    }

    fn resolve_input_transport(
        &self,
        transport: &FrameTransport,
    ) -> io::Result<(String, StdinMode)> {
        match transport {
            FrameTransport::StdoutPipe => Ok((
                "pipe:0".to_string(),
                StdinMode::Transport(FrameTransport::StdoutPipe),
            )),
            FrameTransport::NamedPipe(path) => {
                Ok((path.to_string_lossy().to_string(), StdinMode::Null))
            }
            FrameTransport::LocalSocket(path) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "FFmpeg preprocess does not yet support local-socket input at {:?}.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FFmpeg preprocess cannot consume {:?} as its input transport.",
                    transport
                ),
            )),
        }
    }

    fn output_sink(&self) -> io::Result<String> {
        match &self.output_transport {
            FrameTransport::StdoutPipe => Ok("pipe:1".to_string()),
            FrameTransport::NamedPipe(path) => Ok(path.to_string_lossy().to_string()),
            FrameTransport::LocalSocket(path) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "FFmpeg preprocess does not yet support local-socket output at {:?}.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FFmpeg preprocess cannot emit to {:?}; expected an explicit inter-stage transport.",
                    transport
                ),
            )),
        }
    }
}

impl Preprocessor for FfmpegPreprocessor {
    fn build_preprocess_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let command = self.build_command(context)?;

        Ok(ProcessSpec {
            stage: PipelineStageId::Preprocess,
            program: command.program,
            args: command.args,
            transport: context.transport.clone(),
            stdin: command.stdin,
            stdout: command.stdout,
            stderr_piped: true,
            current_dir: None,
            env: Vec::new(),
            readiness_policy: PipelineStageReadinessPolicy::ReadyOnSpawn,
            log_label: "ffmpeg-preprocess".to_string(),
            stderr_mode: StderrMode::Raw,
            kill_on_drop: true,
            hidden_window: true,
        })
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
        FrameTransport, Preprocessor, SessionOutputPaths, StageRuntimeContext, StdinMode,
        StdoutMode, TransportConfig,
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
        assert_eq!(
            command.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(
            command.stdout,
            StdoutMode::Transport(FrameTransport::StdoutPipe)
        );
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
    fn build_preprocess_spec_keeps_stage_identity_for_named_pipe_output() {
        let named_pipe =
            FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-preprocess-session-1"));
        let preprocessor =
            FfmpegPreprocessor::new("ffmpeg").with_output_transport(named_pipe.clone());
        let spec = preprocessor
            .build_preprocess_spec(&context(
                vec![VideoOp::NormalizeInput],
                FrameTransport::StdoutPipe,
                named_pipe,
            ))
            .unwrap();

        assert_eq!(spec.stage, crate::runtime::PipelineStageId::Preprocess);
        assert_eq!(spec.stdin, StdinMode::Transport(FrameTransport::StdoutPipe));
        assert_eq!(spec.stdout, StdoutMode::Null);
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
