use std::io;
use std::path::PathBuf;

use crate::graph::{
    BackendFamily, BackendFamilyCapabilities, EncoderBinaryAvailability, FeatureAvailability,
    IntermediateExecutionPlan, IntermediateOpOwner, IntermediateOperation, NativeAcceleratorKind,
};
use crate::runtime::{
    BackendExecutor, ExecutorSpec, FrameTransport, LegacyStartupMode, PipelineStageId,
    PipelineStageReadinessPolicy, ProcessSpec, StageRuntimeContext, StderrMode, StdinMode,
    StdoutMode,
};

use super::{
    append_rigaya_denoise_args, build_rigaya_input_args, resolve_encoder_input, EncoderInput,
    UniversalExecutor, UniversalExecutorContext,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmdNativeUpscaleSupport {
    #[default]
    Unsupported,
    VceAmfFsr,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AmdExecutorCapabilities {
    pub vceenc_available: bool,
    pub native_upscale: AmdNativeUpscaleSupport,
}

impl AmdExecutorCapabilities {
    /// Create `AmdExecutorCapabilities` from a backend capability profile.
    ///
    /// Determines whether the AMD VCEEncC encoder binary is explicitly available and, when available,
    /// whether the backend advertises a resize feature that permits AMD native upscale support.
    ///
    ///
    /// # Parameters
    ///
    /// * `profile` - The backend capability profile used to derive executor capabilities.
    ///
    /// # Returns
    ///
    /// An `AmdExecutorCapabilities` value with:
    /// * `vceenc_available` set to `true` only if `profile.family` is `BackendFamily::Amd` and
    ///   `profile.binary_availability` is `EncoderBinaryAvailability::Available`.
    /// * `native_upscale` set to `AmdNativeUpscaleSupport::VceAmfFsr` only when `vceenc_available` is
    ///   `true` and `profile.resize` is `FeatureAvailability::Approximate` or `FeatureAvailability::Exact`;
    ///   otherwise `AmdNativeUpscaleSupport::Unsupported`.
    pub fn from_backend_capability_profile(profile: &BackendFamilyCapabilities) -> Self {
        // Require the binary to be explicitly confirmed available — `Unknown`
        // means the backend was not probed or was substituted (e.g. ffmpeg was
        // passed as encoder_backend), so we must not claim VCEEncC is present.
        let vceenc_available = profile.family == BackendFamily::Amd
            && profile.binary_availability == EncoderBinaryAvailability::Available;
        let native_upscale = if vceenc_available
            && matches!(
                profile.resize,
                FeatureAvailability::Approximate | FeatureAvailability::Exact
            ) {
            AmdNativeUpscaleSupport::VceAmfFsr
        } else {
            AmdNativeUpscaleSupport::Unsupported
        };

        Self {
            vceenc_available,
            native_upscale,
        }
    }

    /// Indicates whether AMD native upscale (AMF/FSR) is available for use.
    ///
    /// # Returns
    ///
    /// `true` if `native_upscale` is not `AmdNativeUpscaleSupport::Unsupported`, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn supports_native_upscale(&self) -> bool {
        !matches!(self.native_upscale, AmdNativeUpscaleSupport::Unsupported)
    }
}

/// What the AMD VCEEncC command renderer has decided to encode.
///
/// Built from the intermediate plan plus capability flags; deliberately narrow
/// — only the operations AMD actually supports today are reflected here.
#[derive(Debug, Clone, PartialEq, Default)]
struct PlannedVceEncOptions {
    /// Target resolution when a resize op is executor-owned.  `None` means
    /// frames arrive pre-scaled from the normalizer or preprocessor.
    output_resolution: Option<String>,
    /// Emit `--vpp-resize amf_fsr` (AMF FSR upscale) when the executor owns
    /// the resize and the probed capability confirms VPP-resize support.
    enable_vpp_resize: bool,
    /// CAS sharpening strength forwarded from `AmdExecutorContext`. 0.0 = disabled.
    cas_strength: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AmdExecutorCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: StdinMode,
    pub stdout: StdoutMode,
}

#[derive(Debug, Clone)]
pub struct AmdExecutorContext {
    pub intermediate_plan: IntermediateExecutionPlan,
    pub native_encoder_backend: String,
    pub native_encoder_path: PathBuf,
    pub native_capabilities: AmdExecutorCapabilities,
    pub fallback_executor: UniversalExecutorContext,
    /// Apply GPU-side denoising equivalent to FFmpeg `hqdn3d` when this
    /// executor reads from the source directly instead of a staged normalizer.
    pub denoise: bool,
    /// CAS sharpening strength applied via `--vpp-unsharp` on the native VCEEncC path.
    /// Range [0.0, 1.0]; 0.0 disables. The Universal fallback path reads cas_strength
    /// from its own `UniversalExecutorContext`.
    pub cas_strength: f32,
}

pub struct AmdExecutor {
    context: AmdExecutorContext,
    fallback: UniversalExecutor,
}

impl AmdExecutor {
    /// Creates a new AmdExecutor that owns the given context and constructs a UniversalExecutor fallback from the context's fallback_executor.
    ///
    /// # Examples
    ///
    pub fn new(context: AmdExecutorContext) -> Self {
        let fallback = UniversalExecutor::new(context.fallback_executor.clone());
        Self { context, fallback }
    }

    /// Builds the command line and IO configuration to invoke the VCEEncC native encoder for the current staged pipeline.
    ///
    /// This constructs the program path, argument vector and stdin/stdout modes based on the stage runtime context,
    /// the selected startup mode, the mapped intermediate execution plan (e.g., `--output-res` / `--vpp-resize`),
    /// profile-based codec/preset/bitrate/GOP settings and the chosen output transport. Errors from resolving encoder
    /// input or output arguments are propagated as `io::Error`.
    ///
    /// # Returns
    ///
    /// An `AmdExecutorCommand` containing the encoder program path, assembled arguments, and configured stdin/stdout modes.
    ///
    /// # Examples
    ///
    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
        startup_mode: LegacyStartupMode,
    ) -> io::Result<AmdExecutorCommand> {
        let profile = &self.context.fallback_executor.profile;
        let (input, stdin) = resolve_encoder_input(context)?;
        let planned = self.map_intermediate_plan();
        let mut args = build_rigaya_input_args(&input, startup_mode.uses_low_latency_input());
        let (output_args, stdout) = self.build_output_args(context)?;

        if self.context.denoise && matches!(&input, EncoderInput::SourceUrl { .. }) {
            append_rigaya_denoise_args(&mut args);
        }

        if let Some(resolution) = &planned.output_resolution {
            args.push("--output-res".to_string());
            args.push(resolution.clone());
        }

        if planned.enable_vpp_resize {
            args.push("--vpp-resize".to_string());
            args.push("amf_fsr".to_string());
        }

        if planned.cas_strength > 0.0 {
            args.push("--vpp-unsharp".to_string());
            args.push(format!(
                "radius=3,weight={:.2},threshold=10.0",
                planned.cas_strength
            ));
        }

        args.push("--profile".to_string());
        args.push("main".to_string());

        // GOP length: align keyframes to HLS segment boundaries so the packager
        // can always cut at a keyframe on every segment edge.
        let source_fps = context.source.metadata.as_ref().and_then(|m| m.source_fps);
        let output_fps =
            crate::pipeline::planned_target_frame_rate(source_fps, &context.execution_plan)
                .unwrap_or(60.0);
        let gop_len = (output_fps * profile.hls_time as f64).round() as u32;

        args.extend([
            "-c".to_string(),
            "hevc".to_string(),
            "--preset".to_string(),
            translate_vceenc_preset(&profile.preset),
            "--vbr".to_string(),
            profile.bitrate.to_string(),
            "--max-bitrate".to_string(),
            profile.max_bitrate.to_string(),
            "--bframes".to_string(),
            profile.bframes.to_string(),
            "--pa".to_string(),
            format!("lookahead={}", profile.lookahead),
            "--gop-len".to_string(),
            gop_len.to_string(),
        ]);

        args.extend(output_args);

        Ok(AmdExecutorCommand {
            program: self.context.native_encoder_path.clone(),
            args,
            stdin,
            stdout,
        })
    }

    /// Maps the intermediate plan to the set of VCEEncC options to render.
    ///
    /// Only executor-owned operations that VCEEncC actually supports today are
    /// acted on. Native resize is enabled only when the intermediate binding
    /// explicitly carries the `AmdVppResize` accelerator annotation; capability
    /// flags alone are not enough. HDR, interpolation, and shader upscale
    /// remain unsupported for the AMD native path and are silently passed over
    /// — they are either handled upstream (shared preprocess) or deferred.
    fn map_intermediate_plan(&self) -> PlannedVceEncOptions {
        let mut planned = PlannedVceEncOptions::default();

        for operation in &self.context.intermediate_plan.operations {
            match operation {
                IntermediateOperation::NormalizeInput(_) => {}
                IntermediateOperation::Resize(resize) => {
                    if resize.binding.owner == IntermediateOpOwner::Executor {
                        planned.output_resolution = Some(resize.plan.target_resolution.clone());
                        if resize.binding.accelerator.as_ref().map(|plan| plan.kind)
                            == Some(NativeAcceleratorKind::AmdVppResize)
                            && self.context.native_capabilities.supports_native_upscale()
                        {
                            planned.enable_vpp_resize = true;
                        }
                    }
                }
                // Not yet supported natively on VCEEncC in the current
                // capability model — handled by shared preprocess or deferred.
                IntermediateOperation::Interpolate(_) => {}
                IntermediateOperation::HdrMetadata(_) => {}
                IntermediateOperation::HdrTransform(_) => {}
                IntermediateOperation::Anime4k2xUpscale(_) => {}
                IntermediateOperation::Artcnn2xUpscale(_) => {}
            }
        }

        planned.cas_strength = self.context.cas_strength;
        planned
    }

    /// Build VCEEncC output/muxing arguments and the corresponding stdout routing for the executor.
    ///
    /// Produces the command-line arguments required to emit encoded output according to the execution
    /// transport, and returns those args together with the `StdoutMode` that the caller should use.
    /// - HLS: returns inline HLS mux options and `StdoutMode::Null`.
    /// - StdoutPipe: returns MPEG-TS args targeting `-` and `StdoutMode::Transport(FrameTransport::StdoutPipe)`.
    /// - NamedPipe(path): returns MPEG-TS args targeting the named pipe and `StdoutMode::Null`.
    ///
    /// # Errors
    /// Returns an `Err(io::Error)` with `io::ErrorKind::Unsupported` for `FrameTransport::LocalSocket`
    /// and `io::ErrorKind::InvalidInput` for other unsupported transports.
    ///
    /// # Examples
    ///
    fn build_output_args(
        &self,
        context: &StageRuntimeContext,
    ) -> io::Result<(Vec<String>, StdoutMode)> {
        let profile = &self.context.fallback_executor.profile;
        match &context.transport.output {
            FrameTransport::HlsOutput { playlist_path, .. } => Ok((
                // Inline HLS muxing — compatibility path only.  The staged
                // pipeline hands MPEG-TS to the common FFmpeg packager instead,
                // but legacy launches still use this path.
                vec![
                    "--audio-codec".to_string(),
                    "aac".to_string(),
                    "--format".to_string(),
                    "hls".to_string(),
                    "--mux-option".to_string(),
                    format!("hls_time:{}", profile.hls_time),
                    "--mux-option".to_string(),
                    "hls_list_size:8".to_string(),
                    "--mux-option".to_string(),
                    "hls_flags:delete_segments".to_string(),
                    "-o".to_string(),
                    playlist_path.to_string_lossy().to_string(),
                ],
                StdoutMode::Null,
            )),
            FrameTransport::StdoutPipe => Ok((
                vec![
                    "--audio-codec".to_string(),
                    "aac".to_string(),
                    "--format".to_string(),
                    "mpegts".to_string(),
                    "-o".to_string(),
                    "-".to_string(),
                ],
                StdoutMode::Transport(FrameTransport::StdoutPipe),
            )),
            FrameTransport::NamedPipe(path) => Ok((
                vec![
                    "--audio-codec".to_string(),
                    "aac".to_string(),
                    "--format".to_string(),
                    "mpegts".to_string(),
                    "-o".to_string(),
                    path.to_string_lossy().to_string(),
                ],
                StdoutMode::Null,
            )),
            FrameTransport::LocalSocket(path) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "AMD VCEEncC executor does not yet support local-socket output at {:?}.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "AMD VCEEncC executor cannot emit {:?} as its output transport.",
                    transport
                ),
            )),
        }
    }
}

/// Map NVEncC-style preset identifiers (e.g., "p1".."p7") to VCEEncC preset names.
///
/// "p1" and "p2" map to `"fast"`, "p6" and "p7" map to `"slow"`, and all other inputs map to `"balanced"`.
///
/// # Examples
///
fn translate_vceenc_preset(preset: &str) -> String {
    match preset {
        "p1" | "p2" => "fast".to_string(),
        "p6" | "p7" => "slow".to_string(),
        _ => "balanced".to_string(),
    }
}

impl BackendExecutor for AmdExecutor {
    /// Selects and builds the executor process specification for the current attempt.
    ///
    /// When the AMD native VCEEncC encoder is available and this is the first attempt,
    /// returns an `ExecutorSpec` configured to run the native AMD encoder (low-latency startup).
    /// Otherwise, delegates to the configured fallback executor and returns its spec.
    ///
    /// # Returns
    ///
    /// An `ExecutorSpec` ready to spawn the chosen encoder process (native AMD VCEEncC on attempt 1 when available; fallback executor otherwise).
    ///
    /// # Examples
    ///
    fn build_executor_spec(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> io::Result<ExecutorSpec> {
        // Only use VCEEncC on the first attempt.  If the AMF encoder fails
        // before producing any output (e.g. "Break in task AMFENC: unknown
        // error" on RDNA 4 / RADV on Linux), the pipeline retries and
        // attempt 2 falls through to the universal (VAAPI) fallback below.
        if self.context.native_capabilities.vceenc_available && attempt_number == 1 {
            let startup_mode = LegacyStartupMode::LowLatency;
            let command = self.build_command(context, startup_mode)?;
            return Ok(ExecutorSpec {
                kind: context.execution_plan.executor,
                startup_label: Some(startup_mode.as_str().to_string()),
                process: ProcessSpec {
                    stage: PipelineStageId::Executor,
                    program: command.program,
                    args: command.args,
                    transport: context.transport.clone(),
                    stdin: command.stdin,
                    stdout: command.stdout,
                    stderr_piped: true,
                    current_dir: None,
                    env: Vec::new(),
                    readiness_policy: PipelineStageReadinessPolicy::ReadyOnHeartbeat,
                    log_label: self.context.native_encoder_backend.clone(),
                    stderr_mode: StderrMode::Raw,
                    kill_on_drop: true,
                    hidden_window: true,
                },
            });
        }

        self.fallback.build_executor_spec(context, attempt_number)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        translate_vceenc_preset, AmdExecutor, AmdExecutorCapabilities, AmdExecutorContext,
        AmdNativeUpscaleSupport,
    };
    use crate::exec::universal::UniversalEncodeBackend;
    use crate::exec::UniversalExecutorContext;
    use crate::graph::{
        Anime4k2xPlan, BackendFamily, BackendFamilyCapabilities, ColorPrimariesIntent,
        EncoderBinaryAvailability, ExecutionPlan, ExecutorKind, FeatureAvailability, HdrPlan,
        HdrRequest, HdrTransformKind, HdrTransformPlan, InterpolationDecision, InterpolationPlan,
        LatencyMode, MatrixCoefficientsIntent, OpExecutionStage, OutputBitDepth, ResizePlan,
        TransferCharacteristicIntent, VideoOp,
    };
    use crate::pipeline::EncodingProfile;
    use crate::runtime::{
        BackendExecutor, FrameTransport, LegacyStartupMode, SessionOutputPaths,
        StageRuntimeContext, StdinMode, StdoutMode, TransportConfig,
    };
    use crate::source::{SourceClassification, SourceDescriptor, SourceKind, SourceTransport};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Constructs a StageRuntimeContext populated with the provided video operations and output transport using fixed test-friendly placeholder values.
    ///
    /// This helper creates a context suitable for unit tests: it sets a constant session id, a simple ExecutionPlan
    /// (executor `Universal`, latency `Balanced`, `requires_local_hls_relay = true`), a sample remote HLS SourceDescriptor,
    /// fixed session paths under `"session"`, and `input` transport `StdoutPipe`. The supplied `video_ops` and `output`
    /// transport are embedded into the returned context.
    ///
    /// # Examples
    ///
    fn stage_context(video_ops: Vec<VideoOp>, output: FrameTransport) -> StageRuntimeContext {
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
            transport: TransportConfig {
                input: FrameTransport::StdoutPipe,
                output,
            },
        }
    }

    /// Constructs the default `EncodingProfile` used by the AMD executor.
    ///
    /// The profile sets conservative defaults for bitrate, preset, lookahead, B-frames,
    /// and HLS segment duration appropriate for VCEEncC-based encoding.
    ///
    /// # Examples
    ///
    fn profile() -> EncodingProfile {
        EncodingProfile {
            bitrate: 25_000,
            max_bitrate: 37_500,
            preset: "p4".to_string(),
            lookahead: 8,
            bframes: 3,
            hls_time: 1,
        }
    }

    /// Creates a default `UniversalExecutorContext` suitable for tests, using a VA-API encoder
    /// and a stdout pipe transport with a single NormalizeInput operation in the intermediate plan.
    ///
    /// The returned context is populated with:
    /// - a test profile,
    /// - `ffmpeg` as the ffmpeg program path,
    /// - `UniversalEncodeBackend::Vaapi` with `vaapi_device` set to `/dev/dri/renderD128`,
    /// - an intermediate plan derived from a stage that contains `VideoOp::NormalizeInput` and uses `FrameTransport::StdoutPipe`.
    ///
    /// # Examples
    ///
    fn fallback_executor() -> UniversalExecutorContext {
        let stage_context =
            stage_context(vec![VideoOp::NormalizeInput], FrameTransport::StdoutPipe);
        UniversalExecutorContext {
            profile: profile(),
            ffmpeg_program: PathBuf::from("ffmpeg"),
            intermediate_plan: stage_context.execution_plan.to_intermediate_plan(),
            encode_backend: UniversalEncodeBackend::Vaapi,
            vaapi_device: Some(PathBuf::from("/dev/dri/renderD128")),
            anime4k_shaders_dir: None,
            artcnn_shaders_dir: None,
            denoise: false,
            cas_strength: 0.0,
        }
    }

    /// Constructs an `AmdExecutorCapabilities` value that models an AMD backend with VCEEncC available and native upscale supported.
    ///
    /// This test helper builds a `BackendFamilyCapabilities` for the AMD family with the `VCEEncC` encoder marked as available and resize capability set to `Approximate`, then derives the resulting `AmdExecutorCapabilities`.
    ///
    /// # Examples
    ///
    fn amd_capabilities_with_upscale() -> AmdExecutorCapabilities {
        AmdExecutorCapabilities::from_backend_capability_profile(&BackendFamilyCapabilities {
            family: BackendFamily::Amd,
            binary_name: "VCEEncC".to_string(),
            binary_availability: EncoderBinaryAvailability::Available,
            resize: FeatureAvailability::Approximate,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        })
    }

    /// Creates an `AmdExecutor` configured for tests using the supplied AMD capability flags.
    ///
    /// The returned executor is initialized with a single `NormalizeInput` stage, the native VCEEncC
    /// backend (`"vceenc"` / `"VCEEncC64.exe"`), the provided `native_capabilities`, and a universal
    /// fallback executor.
    ///
    /// # Examples
    ///
    fn amd_executor(native_capabilities: AmdExecutorCapabilities) -> AmdExecutor {
        let stage_ctx = stage_context(vec![VideoOp::NormalizeInput], FrameTransport::StdoutPipe);
        AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: stage_ctx
                .execution_plan
                .to_intermediate_plan()
                .with_amd_native_resize_bindings(native_capabilities.supports_native_upscale()),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities,
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        })
    }

    #[test]
    fn amd_capabilities_only_claim_vceenc_native_upscale_when_backend_and_probe_support_exist() {
        let native = amd_capabilities_with_upscale();
        let portable_only =
            AmdExecutorCapabilities::from_backend_capability_profile(&BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            });

        assert!(native.vceenc_available);
        assert_eq!(native.native_upscale, AmdNativeUpscaleSupport::VceAmfFsr);
        assert!(native.supports_native_upscale());
        assert!(!portable_only.vceenc_available);
        assert_eq!(
            portable_only.native_upscale,
            AmdNativeUpscaleSupport::Unsupported
        );
        assert!(!portable_only.supports_native_upscale());
    }

    #[test]
    fn amd_executor_emits_vceenc_command_when_native_available() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "2560x1440".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
        );
        let executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: context
                .execution_plan
                .to_intermediate_plan()
                .with_amd_native_resize_bindings(true),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: amd_capabilities_with_upscale(),
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        });

        let spec = executor.build_executor_spec(&context, 1).unwrap();

        // Uses the native VCEEncC binary, not ffmpeg.
        assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
        assert_eq!(spec.process.log_label, "vceenc");
        assert_eq!(
            spec.process.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        // Staged output: mpegts to stdout.
        assert_eq!(
            spec.process.stdout,
            StdoutMode::Transport(FrameTransport::StdoutPipe)
        );

        // Core encode args.
        assert!(
            spec.process.args.windows(2).any(|w| w == ["-c", "hevc"]),
            "codec: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--preset", "balanced"]),
            "preset: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--vbr", "25000"]),
            "vbr: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--max-bitrate", "37500"]),
            "max-bitrate: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--bframes", "3"]),
            "bframes: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--pa", "lookahead=8"]),
            "lookahead: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--profile", "main"]),
            "profile: {:?}",
            spec.process.args
        );
        // Resize op is executor-owned → output-res and vpp-resize emitted.
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--output-res", "2560x1440"]),
            "output-res: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--vpp-resize", "amf_fsr"]),
            "vpp-resize: {:?}",
            spec.process.args
        );
        // Staged output: mpegts format to stdout pipe.
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--format", "mpegts"]),
            "output format: {:?}",
            spec.process.args
        );
        assert!(
            spec.process.args.windows(2).any(|w| w == ["-o", "-"]),
            "output sink: {:?}",
            spec.process.args
        );
    }

    #[test]
    fn amd_executor_falls_back_to_universal_when_vceenc_unavailable() {
        let context = stage_context(vec![VideoOp::NormalizeInput], FrameTransport::StdoutPipe);
        let executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: context.execution_plan.to_intermediate_plan(),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            // No vceenc → must fall through to Universal executor.
            native_capabilities: AmdExecutorCapabilities::from_backend_capability_profile(
                &BackendFamilyCapabilities {
                    family: BackendFamily::Universal,
                    binary_name: "ffmpeg".to_string(),
                    binary_availability: EncoderBinaryAvailability::Available,
                    resize: FeatureAvailability::Exact,
                    interpolation: FeatureAvailability::Unavailable,
                    hdr_transform: FeatureAvailability::Exact,
                    metadata_injection: FeatureAvailability::Exact,
                },
            ),
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        });

        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.program, PathBuf::from("ffmpeg"));
        assert!(spec.process.log_label.starts_with("ffmpeg-universal-"));
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["-c:v", "hevc_vaapi"]),
            "expected vaapi fallback: {:?}",
            spec.process.args
        );
        assert!(
            !spec.process.args.iter().any(|a| a == "--vpp-resize"),
            "no vpp-resize in universal fallback"
        );
    }

    #[test]
    fn amd_executor_omits_vpp_resize_when_upscale_unsupported_even_with_resize_op() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
        );
        // VCEEncC available but without VPP-resize capability.
        let no_upscale_caps =
            AmdExecutorCapabilities::from_backend_capability_profile(&BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Unavailable, // ← no VPP-resize
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            });
        let executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: context.execution_plan.to_intermediate_plan(),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: no_upscale_caps,
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        });

        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
        // output-res still emitted (resize op is executor-owned).
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--output-res", "3840x2160"]),
            "output-res: {:?}",
            spec.process.args
        );
        // vpp-resize must NOT appear — capability says it is unavailable.
        assert!(
            !spec.process.args.iter().any(|a| a == "--vpp-resize"),
            "vpp-resize must be absent when capability is Unavailable: {:?}",
            spec.process.args
        );
    }

    #[test]
    fn amd_executor_does_not_infer_native_upscale_from_capability_alone() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
        );
        let executor = AmdExecutor::new(AmdExecutorContext {
            // Deliberately leave the plan unannotated: capability flags alone
            // must not cause the executor to infer native resize ownership.
            intermediate_plan: context.execution_plan.to_intermediate_plan(),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: amd_capabilities_with_upscale(),
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        });

        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--output-res", "3840x2160"]),
            "output-res: {:?}",
            spec.process.args
        );
        assert!(
            !spec.process.args.iter().any(|a| a == "--vpp-resize"),
            "native resize must require an explicit AMD accelerator binding: {:?}",
            spec.process.args
        );
    }

    #[test]
    fn amd_executor_leaves_resize_to_shared_preprocess_when_plan_is_preprocess_owned() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Preprocess,
                }),
            ],
            FrameTransport::StdoutPipe,
        );
        let executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: context
                .execution_plan
                .to_intermediate_plan()
                .with_amd_native_resize_bindings(true),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: amd_capabilities_with_upscale(),
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        });

        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
        assert!(
            !spec.process.args.iter().any(|a| a == "--output-res"),
            "preprocess-owned resize should not be rendered by the AMD executor: {:?}",
            spec.process.args
        );
        assert!(
            !spec.process.args.iter().any(|a| a == "--vpp-resize"),
            "preprocess-owned resize should not emit native AMD resize flags: {:?}",
            spec.process.args
        );
    }

    #[test]
    fn amd_executor_keeps_non_resize_features_explicitly_unsupported() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "2560x1440".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::native_backend(OpExecutionStage::Executor),
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
                VideoOp::Anime4k2xUpscale(Anime4k2xPlan {
                    target_resolution: "2560x1440".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
        );
        let executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: context
                .execution_plan
                .to_intermediate_plan()
                .with_amd_native_resize_bindings(true),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: amd_capabilities_with_upscale(),
            denoise: false,
            cas_strength: 0.0,
            fallback_executor: fallback_executor(),
        });

        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--vpp-resize", "amf_fsr"]),
            "resize should still use AMD native upscale: {:?}",
            spec.process.args
        );
        assert!(!spec.process.args.iter().any(|a| a == "--vpp-fruc"));
        assert!(!spec.process.args.iter().any(|a| a == "--vpp-ngx-truehdr"));
        assert!(!spec.process.args.iter().any(|a| a.contains("ngx-vsr")));
        assert!(
            !spec
                .process
                .args
                .iter()
                .any(|a| a.contains("Anime4K") || a.contains("ArtCNN")),
            "shader upscale must not fake native AMD support: {:?}",
            spec.process.args
        );
    }

    #[test]
    fn amd_executor_applies_gpu_denoise_only_on_source_pull_inputs() {
        let direct_context = StageRuntimeContext {
            transport: TransportConfig {
                input: FrameTransport::SourcePull,
                output: FrameTransport::HlsOutput {
                    playlist_path: PathBuf::from("session\\index.m3u8"),
                    segment_dir: PathBuf::from("session"),
                },
            },
            ..stage_context(vec![VideoOp::NormalizeInput], FrameTransport::StdoutPipe)
        };
        let direct_executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: direct_context.execution_plan.to_intermediate_plan(),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: amd_capabilities_with_upscale(),
            fallback_executor: fallback_executor(),
            denoise: true,
            cas_strength: 0.0,
        });
        let direct_command = direct_executor
            .build_command(&direct_context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(
            direct_command.args.windows(2).any(|w| {
                w == [
                    "--vpp-convolution3d",
                    "ythresh=4,cthresh=3,t_ythresh=6,t_cthresh=4.5",
                ]
            }),
            "source-pull AMD path should apply GPU denoise: {:?}",
            direct_command.args
        );

        let staged_context =
            stage_context(vec![VideoOp::NormalizeInput], FrameTransport::StdoutPipe);
        let staged_executor = AmdExecutor::new(AmdExecutorContext {
            intermediate_plan: staged_context.execution_plan.to_intermediate_plan(),
            native_encoder_backend: "vceenc".to_string(),
            native_encoder_path: PathBuf::from("VCEEncC64.exe"),
            native_capabilities: amd_capabilities_with_upscale(),
            fallback_executor: fallback_executor(),
            denoise: true,
            cas_strength: 0.0,
        });
        let staged_command = staged_executor
            .build_command(&staged_context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(
            !staged_command
                .args
                .iter()
                .any(|arg| arg == "--vpp-convolution3d"),
            "staged inputs should rely on the FFmpeg normalizer for denoise: {:?}",
            staged_command.args
        );
    }

    #[test]
    fn amd_executor_routes_hls_output_to_inline_mux() {
        let hls_output = FrameTransport::HlsOutput {
            playlist_path: PathBuf::from("session\\index.m3u8"),
            segment_dir: PathBuf::from("session"),
        };
        let context = stage_context(vec![VideoOp::NormalizeInput], hls_output);
        let executor = amd_executor(amd_capabilities_with_upscale());
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--format", "hls"]),
            "hls format: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--mux-option", "hls_list_size:8"]),
            "hls_list_size: {:?}",
            spec.process.args
        );
        assert_eq!(spec.process.stdout, StdoutMode::Null);
    }

    #[test]
    fn amd_executor_routes_named_pipe_to_mpegts() {
        let named_pipe =
            FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1"));
        let context = stage_context(vec![VideoOp::NormalizeInput], named_pipe.clone());
        let executor = amd_executor(amd_capabilities_with_upscale());
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--format", "mpegts"]),
            "format: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["-o", r"\\.\pipe\referee-normalized-session-1"]),
            "output path: {:?}",
            spec.process.args
        );
        assert_eq!(spec.process.stdout, StdoutMode::Null);
    }

    #[test]
    fn translate_vceenc_preset_maps_nvenc_style_levels_to_named_presets() {
        assert_eq!(translate_vceenc_preset("p1"), "fast");
        assert_eq!(translate_vceenc_preset("p2"), "fast");
        assert_eq!(translate_vceenc_preset("p3"), "balanced");
        assert_eq!(translate_vceenc_preset("p4"), "balanced");
        assert_eq!(translate_vceenc_preset("p5"), "balanced");
        assert_eq!(translate_vceenc_preset("p6"), "slow");
        assert_eq!(translate_vceenc_preset("p7"), "slow");
        // Unknown presets collapse to balanced.
        assert_eq!(translate_vceenc_preset("balanced"), "balanced");
        assert_eq!(translate_vceenc_preset("fast"), "balanced");
    }
}
