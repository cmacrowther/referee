use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::graph::{
    Anime4k2xPlan, Artcnn2xPlan, ColorPrimariesIntent, ExecutorKind, HdrRequest,
    IntermediateExecutionPlan, IntermediateOpOwner, IntermediateOperation,
    MatrixCoefficientsIntent, OutputBitDepth, TransferCharacteristicIntent,
};
use crate::pipeline::EncodingProfile;
use crate::runtime::{
    BackendExecutor, ExecutorSpec, FrameTransport, LegacyStartupMode, PipelineStageId,
    PipelineStageReadinessPolicy, ProcessSpec, StageRuntimeContext, StderrMode, StdinMode,
    StdoutMode,
};

use super::{resolve_encoder_input, EncoderInput};

const DEFAULT_AUDIO_BITRATE_KBPS: u32 = 160;
const DEFAULT_VAAPI_DEVICE: &str = "/dev/dri/renderD128";
use super::REQUEST_USER_AGENT;
const INPUT_ANALYZE_MICROS: u32 = 4_000_000;
const INPUT_PROBESIZE_BYTES: u32 = 1_000_000;
const GENERATED_RUNTIME_SHADER_NAME: &str = "referee-universal-runtime.glsl";
const CAS_SHADER_STRENGTH_PLACEHOLDER: &str = "{{CAS_STRENGTH}}";
const CAS_SHADER_TEMPLATE: &str = include_str!("../../shaders/cas/Referee_CAS.glsl");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePlatform {
    Windows,
    Linux,
    Other,
}

impl RuntimePlatform {
    pub(crate) fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UniversalBackendSelectionContext<'a> {
    pub platform: RuntimePlatform,
    pub gpu_vendor: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniversalEncodeBackend {
    Nvenc,
    Vaapi,
    /// AMD Advanced Media Framework (Windows-only hardware encoder via FFmpeg `hevc_amf`).
    Amf,
    CpuFallback,
}

impl UniversalEncodeBackend {
    fn encoder_name(self) -> &'static str {
        match self {
            Self::Nvenc => "hevc_nvenc",
            Self::Vaapi => "hevc_vaapi",
            Self::Amf => "hevc_amf",
            Self::CpuFallback => "libx265",
        }
    }

    fn log_label(self) -> &'static str {
        match self {
            Self::Nvenc => "ffmpeg-universal-nvenc",
            Self::Vaapi => "ffmpeg-universal-vaapi",
            Self::Amf => "ffmpeg-universal-amf",
            Self::CpuFallback => "ffmpeg-universal-cpu",
        }
    }

    fn pixel_format(self, output_bit_depth: OutputBitDepth) -> &'static str {
        match self {
            Self::Nvenc | Self::Vaapi | Self::Amf => {
                if output_bit_depth == OutputBitDepth::Bit10 {
                    "p010le"
                } else {
                    "nv12"
                }
            }
            Self::CpuFallback => {
                if output_bit_depth == OutputBitDepth::Bit10 {
                    "yuv420p10le"
                } else {
                    "yuv420p"
                }
            }
        }
    }
}

/// Selects the FFmpeg encode path for the Universal executor.
///
/// Rules stay intentionally narrow and explicit:
/// Linux + NVIDIA  -> NVENC
/// Linux + AMD     -> VAAPI
/// Windows + NVIDIA -> NVENC
/// Windows + AMD   -> AMF (hevc_amf)
/// anything else   -> CPU fallback
pub(crate) fn select_universal_encode_backend(
    context: UniversalBackendSelectionContext<'_>,
) -> UniversalEncodeBackend {
    match (
        context.platform,
        context.gpu_vendor.trim().to_ascii_lowercase().as_str(),
    ) {
        (RuntimePlatform::Linux, "nvidia") => UniversalEncodeBackend::Nvenc,
        (RuntimePlatform::Linux, "amd") => UniversalEncodeBackend::Vaapi,
        (RuntimePlatform::Windows, "nvidia") => UniversalEncodeBackend::Nvenc,
        (RuntimePlatform::Windows, "amd") => UniversalEncodeBackend::Amf,
        _ => UniversalEncodeBackend::CpuFallback,
    }
}

/// Cross-platform FFmpeg-based executor for the Universal path.
///
/// This executor uses libplacebo for portable resize and HDR/color processing,
/// then selects a conservative hardware encode backend per platform/vendor.
/// It is intentionally honest about feature scope: upscale and HDR processing
/// are supported now, while FRUC-style frame generation remains deferred.
#[derive(Debug, Clone)]
pub struct UniversalExecutorContext {
    pub profile: EncodingProfile,
    pub ffmpeg_program: PathBuf,
    pub intermediate_plan: IntermediateExecutionPlan,
    pub encode_backend: UniversalEncodeBackend,
    pub vaapi_device: Option<PathBuf>,
    /// Directory containing Anime4K GLSL shader files (e.g. `lib/shaders/anime4k/`).
    /// When `None`, Anime4K upscale ops fall back to passthrough without custom shaders.
    pub anime4k_shaders_dir: Option<PathBuf>,
    /// Directory containing ArtCNN GLSL shader files (e.g. `lib/shaders/artcnn/`).
    /// When `None`, ArtCNN upscale ops fall back to passthrough without custom shaders.
    pub artcnn_shaders_dir: Option<PathBuf>,
    /// Apply FFmpeg `hqdn3d` when this executor reads from the source directly
    /// instead of receiving already-normalized frames from the staged path.
    pub denoise: bool,
    /// CAS (FidelityFX Contrast Adaptive Sharpening) strength applied via a
    /// generated libplacebo custom shader. Range [0.0, 1.0]; 0.0 disables sharpening.
    pub cas_strength: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UniversalExecutorCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: StdinMode,
    pub stdout: StdoutMode,
    pub current_dir: Option<PathBuf>,
}

pub struct UniversalExecutor {
    context: UniversalExecutorContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedCustomShader {
    working_dir: PathBuf,
    shader_name: String,
}

#[derive(Debug, Clone, PartialEq)]
struct UniversalFilterPlan {
    output_resolution: Option<String>,
    resize_quality: Option<u8>,
    output_bit_depth: OutputBitDepth,
    color_primaries: ColorPrimariesIntent,
    transfer: TransferCharacteristicIntent,
    matrix: MatrixCoefficientsIntent,
    tone_map_to_hdr10: bool,
    deferred_interpolation: bool,
    /// True when the HDR transform (bit-depth conversion + colour-space
    /// re-tagging) was assigned to the preprocess stage and has already been
    /// rendered by `FfmpegPreprocessor` before frames reach this executor.
    /// When set, `has_libplacebo_processing` stops counting `output_bit_depth
    /// == Bit10` as a reason to emit a libplacebo filter: the frames that
    /// arrive here are already in the correct pixel format, so the encoder's
    /// backend-tail format conversion is sufficient.
    hdr_preprocess_owned: bool,
    /// Resolved paths to custom GLSL shader files. Non-empty when a shader
    /// upscale op is present and the corresponding shader directory exists.
    custom_shader_paths: Vec<PathBuf>,
    /// Apply `hqdn3d` before executor-side filtering when this path reads the
    /// source directly rather than using the staged FFmpeg normalizer.
    denoise: bool,
    /// CAS strength forwarded from `UniversalExecutorContext`. 0.0 = disabled.
    cas_strength: f32,
}

impl Default for UniversalFilterPlan {
    /// Constructs a default UniversalFilterPlan with no filter or shader work requested and an 8-bit, source-preserving color intent.
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
            deferred_interpolation: false,
            hdr_preprocess_owned: false,
            custom_shader_paths: Vec::new(),
            denoise: false,
            cas_strength: 0.0,
        }
    }
}

impl UniversalFilterPlan {
    /// Builds a `UniversalFilterPlan` representing the executor-owned operations described
    /// in an `IntermediateExecutionPlan`.
    ///
    /// The returned plan reflects only operations the Universal executor should perform
    /// (executor-owned operations); preprocess-owned operations are not applied. This
    /// function also asserts in debug builds that the intermediate plan targets the
    /// Universal executor.
    ///
    /// # Parameters
    ///
    /// - `intermediate_plan`: Source intermediate execution plan to convert.
    /// - `anime4k_shaders_dir`: Optional directory where Anime4k GLSL shaders live; used
    ///   to resolve executor-owned Anime4k upscale shader paths.
    /// - `artcnn_shaders_dir`: Optional directory where ArtCNN GLSL shaders live; used
    ///   to resolve executor-owned ArtCNN upscale shader paths.
    ///
    /// # Returns
    ///
    /// A `UniversalFilterPlan` that encodes resize, HDR, color, and custom-shader intents
    /// the Universal executor is responsible for.
    ///
    /// # Examples
    ///
    fn from_intermediate_plan(
        intermediate_plan: &IntermediateExecutionPlan,
        anime4k_shaders_dir: Option<&Path>,
        artcnn_shaders_dir: Option<&Path>,
    ) -> Self {
        let mut plan = Self::default();

        debug_assert_eq!(intermediate_plan.executor, ExecutorKind::Universal);

        for operation in &intermediate_plan.operations {
            plan.apply_intermediate_operation(operation, anime4k_shaders_dir, artcnn_shaders_dir);
        }

        plan
    }

    /// Constructs a test-only `UniversalFilterPlan` from a legacy list of `VideoOp`s.
    ///
    /// This is a helper used in unit tests to convert a slice of `VideoOp` into an
    /// `IntermediateExecutionPlan` and then into a `UniversalFilterPlan`, wiring the
    /// provided optional shader directories for executor-owned upscales.
    ///
    /// # Examples
    ///
    #[cfg(test)]
    fn from_video_ops(
        video_ops: &[crate::graph::VideoOp],
        anime4k_shaders_dir: Option<&Path>,
        artcnn_shaders_dir: Option<&Path>,
    ) -> Self {
        use crate::graph::{ExecutionPlan, LatencyMode};

        Self::from_intermediate_plan(
            &ExecutionPlan {
                executor: ExecutorKind::Universal,
                latency_mode: LatencyMode::Balanced,
                requires_local_hls_relay: true,
                video_ops: video_ops.to_vec(),
            }
            .to_intermediate_plan(),
            anime4k_shaders_dir,
            artcnn_shaders_dir,
        )
    }

    /// Incorporates a single intermediate operation into the current filter plan, honoring operation ownership.
    ///
    /// Applies only the parts of `operation` that the executor is responsible for and updates
    /// internal flags such as `output_bit_depth`, `deferred_interpolation`, and `hdr_preprocess_owned`.
    /// When an upscale operation owned by the executor is applied, the corresponding shader directory
    /// (`anime4k_shaders_dir` or `artcnn_shaders_dir`) is used to resolve shader file paths.
    ///
    /// # Parameters
    ///
    /// - `operation`: the intermediate operation to apply; only executor-owned portions are acted on.
    /// - `anime4k_shaders_dir`: directory containing Anime4k GLSL shaders; used when applying executor-owned Anime4k upscales.
    /// - `artcnn_shaders_dir`: directory containing ArtCNN GLSL shaders; used when applying executor-owned ArtCNN upscales.
    ///
    /// # Examples
    ///
    fn apply_intermediate_operation(
        &mut self,
        operation: &IntermediateOperation,
        anime4k_shaders_dir: Option<&Path>,
        artcnn_shaders_dir: Option<&Path>,
    ) {
        match operation {
            IntermediateOperation::NormalizeInput(_) => {}
            IntermediateOperation::Resize(resize) => {
                if resize.binding.owner == IntermediateOpOwner::Executor {
                    self.apply_resize_plan(&resize.plan);
                }
            }
            IntermediateOperation::Interpolate(interpolation) => {
                if interpolation.plan.is_enabled()
                    && interpolation.binding.owner != IntermediateOpOwner::SharedPreprocess
                {
                    self.deferred_interpolation = true;
                }
            }
            IntermediateOperation::HdrMetadata(metadata) => {
                // Metadata-only HDR requests still carry output-format intent
                // through the split intermediate representation, so preserve
                // the existing behaviour by applying the shared HDR request
                // here unless a later preprocess-owned transform clears it.
                self.apply_hdr_plan(&metadata.hdr);
            }
            IntermediateOperation::HdrTransform(transform) => {
                self.output_bit_depth = transform.hdr.output_bit_depth;
                if transform.binding.owner == IntermediateOpOwner::SharedPreprocess {
                    self.mark_hdr_preprocess_owned(transform.hdr.output_bit_depth);
                } else {
                    self.apply_hdr_plan(&transform.hdr);
                }
            }
            IntermediateOperation::Anime4k2xUpscale(shader) => {
                if shader.binding.owner == IntermediateOpOwner::Executor {
                    self.apply_anime4k_plan(&shader.plan, anime4k_shaders_dir);
                }
            }
            IntermediateOperation::Artcnn2xUpscale(shader) => {
                if shader.binding.owner == IntermediateOpOwner::Executor {
                    self.apply_artcnn_plan(&shader.plan, artcnn_shaders_dir);
                }
            }
        }
    }

    fn apply_resize_plan(&mut self, resize: &crate::graph::ResizePlan) {
        self.output_resolution = Some(resize.target_resolution.clone());
        self.resize_quality = resize.quality;
    }

    fn apply_anime4k_plan(&mut self, plan: &Anime4k2xPlan, shaders_dir: Option<&Path>) {
        self.output_resolution = Some(plan.target_resolution.clone());
        if let Some(dir) = shaders_dir {
            self.custom_shader_paths = vec![dir.join("Anime4K_Upscale_CNN_x2_M.glsl")];
        }
    }

    /// Applies an ArtCNN 2x upscale plan to the filter plan.
    ///
    /// Sets the plan's output resolution to the upscale target and, if a shader
    /// directory is provided, records the resolved ArtCNN shader file path.
    ///
    /// # Examples
    ///
    fn apply_artcnn_plan(&mut self, plan: &Artcnn2xPlan, shaders_dir: Option<&Path>) {
        self.output_resolution = Some(plan.target_resolution.clone());
        if let Some(dir) = shaders_dir {
            self.custom_shader_paths = vec![dir.join("ArtCNN_C4F16.glsl")];
        }
    }

    /// Apply an HDR transform plan to this filter plan.
    ///
    /// Updates the plan's output bit depth, color primaries, transfer, and matrix
    /// to match the provided HDR plan, clears any `hdr_preprocess_owned` marker,
    /// and sets `tone_map_to_hdr10` when the HDR plan requests tonemapping to HDR10.
    ///
    /// # Examples
    ///
    fn apply_hdr_plan(&mut self, hdr: &crate::graph::HdrPlan) {
        self.hdr_preprocess_owned = false;
        self.output_bit_depth = hdr.output_bit_depth;
        self.color_primaries = hdr.color_primaries;
        self.transfer = hdr.transfer;
        self.matrix = hdr.matrix;
        self.tone_map_to_hdr10 = matches!(hdr.request, HdrRequest::TonemapToHdr10);
    }

    /// Marks that HDR bit-depth conversion and related transforms were already applied in a shared preprocess stage.
    ///
    /// Sets the plan's output bit depth to `output_bit_depth`, resets color/transfer/matrix intents to
    /// `PreserveSource`, disables HDR tonemapping, and records that HDR processing is owned by the preprocess stage.
    ///
    /// # Examples
    ///
    fn mark_hdr_preprocess_owned(&mut self, output_bit_depth: OutputBitDepth) {
        self.output_bit_depth = output_bit_depth;
        self.color_primaries = ColorPrimariesIntent::PreserveSource;
        self.transfer = TransferCharacteristicIntent::PreserveSource;
        self.matrix = MatrixCoefficientsIntent::PreserveSource;
        self.tone_map_to_hdr10 = false;
        self.hdr_preprocess_owned = true;
    }

    /// Determines whether the executor must perform libplacebo-based processing.
    ///
    /// Returns `true` when any executor-side libplacebo work is required: custom shader upscaling, CAS sharpening, an explicit output resolution/resize, tonemapping to HDR10, a required 10-bit conversion when the preprocess stage did not already produce 10-bit frames, or any color/transfer/matrix intent that is not `PreserveSource`. Returns `false` when none of these conditions apply.
    ///
    /// # Examples
    ///
    fn has_libplacebo_processing(&self) -> bool {
        !self.custom_shader_paths.is_empty()
            || self.cas_strength > 0.0
            || self.output_resolution.is_some()
            || self.tone_map_to_hdr10
            // When the preprocess stage already converted to 10-bit and
            // delivered the frames in the correct pixel format, the executor
            // does not need a libplacebo filter solely for bit-depth.  The
            // backend-tail `format=` conversion (e.g. `p010le` for NVENC) is
            // still emitted via `build_backend_filter_tail`.
            || (self.output_bit_depth == OutputBitDepth::Bit10 && !self.hdr_preprocess_owned)
            || !matches!(self.color_primaries, ColorPrimariesIntent::PreserveSource)
            || !matches!(self.transfer, TransferCharacteristicIntent::PreserveSource)
            || !matches!(self.matrix, MatrixCoefficientsIntent::PreserveSource)
    }

    fn build_filter_graph(
        &self,
        encode_backend: UniversalEncodeBackend,
        custom_shader: Option<&ResolvedCustomShader>,
    ) -> Option<String> {
        let libplacebo_filter = build_libplacebo_filter(self, custom_shader);
        let mut filters = Vec::new();

        if self.denoise {
            filters.push("hqdn3d".to_string());
        }

        if let Some(filter) = libplacebo_filter.as_deref() {
            filters.push(filter.to_string());
        }

        if let Some(tail) = Self::build_backend_filter_tail(
            encode_backend,
            self.output_bit_depth,
            libplacebo_filter.is_some(),
        ) {
            filters.push(tail);
        }

        if filters.is_empty() {
            None
        } else {
            Some(filters.join(","))
        }
    }

    fn build_backend_filter_tail(
        encode_backend: UniversalEncodeBackend,
        output_bit_depth: OutputBitDepth,
        has_libplacebo_filter: bool,
    ) -> Option<String> {
        match encode_backend {
            UniversalEncodeBackend::Nvenc if !has_libplacebo_filter => Some(format!(
                "format={}",
                encode_backend.pixel_format(output_bit_depth)
            )),
            UniversalEncodeBackend::Vaapi => Some(format!(
                "format={},hwupload",
                encode_backend.pixel_format(output_bit_depth)
            )),
            // AMF accepts NV12/P010LE software frames; always add the format
            // conversion even when libplacebo is present (libplacebo outputs
            // yuv420p/yuv420p10le which AMF cannot consume directly).
            UniversalEncodeBackend::Amf => Some(format!(
                "format={}",
                encode_backend.pixel_format(output_bit_depth)
            )),
            UniversalEncodeBackend::CpuFallback if !has_libplacebo_filter => Some(format!(
                "format={}",
                encode_backend.pixel_format(output_bit_depth)
            )),
            _ => None,
        }
    }
}

impl UniversalExecutor {
    pub fn new(context: UniversalExecutorContext) -> Self {
        Self { context }
    }

    /// Build the ffmpeg command and associated I/O modes for running the universal executor.
    ///
    /// The returned command contains the program path, argument vector, and configured stdin/stdout
    /// transport ready to spawn an ffmpeg process that performs the executor's planned work
    /// (encoding, optional libplacebo filter graph, audio encoding and chosen output transport).
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if assembling the output transport arguments fails (for example when
    /// the requested frame transport is unsupported or invalid).
    ///
    /// # Examples
    ///
    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
        startup_mode: LegacyStartupMode,
    ) -> io::Result<UniversalExecutorCommand> {
        let (input, stdin) = resolve_encoder_input(context)?;

        // Plan filters now so we can conditionally inject Vulkan init flags
        // before the input args (FFmpeg requires global device options to
        // precede the first -i).
        let filter_plan = self.plan_filters(context);
        let resolved_custom_shader = self.materialize_custom_shader(&filter_plan, context)?;

        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-nostdin".to_string(),
            // Emit structured per-frame progress to stderr and suppress the
            // interactive overwriting stats line.  The supervisor parses the
            // key=value blocks and formats them into NVEncC-style log lines.
            "-progress".to_string(),
            "pipe:2".to_string(),
            "-nostats".to_string(),
        ];

        // When libplacebo filter processing is active and a hardware encode
        // backend is selected, initialise a Vulkan device so libplacebo can
        // run shaders on the GPU instead of the CPU.  The named device "vk"
        // is referenced by -filter_hw_device so the filter graph picks it up.
        if filter_plan.has_libplacebo_processing()
            && self.context.encode_backend != UniversalEncodeBackend::CpuFallback
        {
            args.extend([
                "-init_hw_device".to_string(),
                "vulkan=vk:0".to_string(),
                "-filter_hw_device".to_string(),
                "vk".to_string(),
            ]);
        }

        // Offload video decode to the GPU to free up CPU cycles.  Only applied
        // when reading directly from a source URL; piped inputs (preprocess
        // stage output) carry already-decoded frames in NUT/raw format where a
        // hardware decoder brings no benefit.
        // `-hwaccel_output_format nv12` copies decoded frames back to system
        // memory so all downstream filters (including libplacebo/Vulkan) can
        // consume them without requiring explicit device interop.
        if matches!(input, EncoderInput::SourceUrl { .. }) {
            match (RuntimePlatform::current(), self.context.encode_backend) {
                (RuntimePlatform::Windows, UniversalEncodeBackend::Nvenc)
                | (RuntimePlatform::Windows, UniversalEncodeBackend::Amf) => {
                    args.extend([
                        "-hwaccel".to_string(),
                        "d3d11va".to_string(),
                        "-hwaccel_output_format".to_string(),
                        "nv12".to_string(),
                    ]);
                }
                (RuntimePlatform::Linux, UniversalEncodeBackend::Nvenc) => {
                    args.extend([
                        "-hwaccel".to_string(),
                        "cuda".to_string(),
                        "-hwaccel_output_format".to_string(),
                        "nv12".to_string(),
                    ]);
                }
                _ => {}
            }
        }

        args.extend(build_ffmpeg_input_args(
            &input,
            startup_mode.uses_low_latency_input(),
        ));

        debug_assert_eq!(
            self.context.intermediate_plan.executor,
            context.execution_plan.executor
        );
        let output_bit_depth = filter_plan.output_bit_depth;
        let filter_graph = self.build_filter_graph(&filter_plan, resolved_custom_shader.as_ref());
        let gop_len = self.derive_gop_length(context);
        let (output_args, stdout) = self.build_output_args(context)?;

        if self.context.encode_backend == UniversalEncodeBackend::Vaapi {
            args.push("-vaapi_device".to_string());
            args.push(
                self.context
                    .vaapi_device
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_VAAPI_DEVICE))
                    .to_string_lossy()
                    .to_string(),
            );
        }

        args.extend([
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
        ]);

        if let Some(filter_graph) = filter_graph {
            args.push("-vf".to_string());
            args.push(filter_graph);
        }

        args.extend(self.build_video_encode_args(output_bit_depth, gop_len));
        args.extend([
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            format!("{}k", DEFAULT_AUDIO_BITRATE_KBPS),
        ]);
        args.extend(output_args);

        Ok(UniversalExecutorCommand {
            program: self.context.ffmpeg_program.clone(),
            args,
            stdin,
            stdout,
            current_dir: resolved_custom_shader.map(|shader| shader.working_dir),
        })
    }

    /// Builds a UniversalFilterPlan from the executor's intermediate plan.
    ///
    /// This converts the ownership-aware IntermediateExecutionPlan into a UniversalFilterPlan that represents
    /// only executor-owned filter work (e.g., executor-owned resizes, shader upscales, and tone-mapping).
    /// Preprocess-owned operations are intentionally ignored here so they are not duplicated by the executor.
    fn plan_filters(&self, context: &StageRuntimeContext) -> UniversalFilterPlan {
        // Consume the ownership-aware intermediate plan rather than raw
        // graph ops so this executor behaves like a stage-owned renderer:
        // only executor-owned work is interpreted here, while preprocess-owned
        // work is left to `FfmpegPreprocessor`.
        let mut plan = UniversalFilterPlan::from_intermediate_plan(
            &self.context.intermediate_plan,
            self.context.anime4k_shaders_dir.as_deref(),
            self.context.artcnn_shaders_dir.as_deref(),
        );
        plan.denoise =
            self.context.denoise && matches!(&context.transport.input, FrameTransport::SourcePull);
        plan.cas_strength = self.context.cas_strength;
        plan
    }

    /// Builds the FFmpeg filter graph string for the provided `UniversalFilterPlan`.
    ///
    /// Calls the plan's builder with the executor's selected encode backend and returns
    /// the composed filter graph if any processing is required by the plan.
    ///
    /// # Returns
    ///
    /// `Some` containing the filter graph string when filters are required, or `None` when
    /// no filter graph should be applied.
    ///
    /// # Examples
    ///
    fn build_filter_graph(
        &self,
        plan: &UniversalFilterPlan,
        custom_shader: Option<&ResolvedCustomShader>,
    ) -> Option<String> {
        plan.build_filter_graph(self.context.encode_backend, custom_shader)
    }

    fn materialize_custom_shader(
        &self,
        plan: &UniversalFilterPlan,
        context: &StageRuntimeContext,
    ) -> io::Result<Option<ResolvedCustomShader>> {
        if plan.custom_shader_paths.is_empty() && plan.cas_strength <= 0.0 {
            return Ok(None);
        }

        let needs_generated_shader = plan.cas_strength > 0.0 || plan.custom_shader_paths.len() > 1;
        if !needs_generated_shader {
            let shader = &plan.custom_shader_paths[0];
            let shader_name = shader
                .file_name()
                .unwrap_or(shader.as_os_str())
                .to_string_lossy()
                .to_string();
            let working_dir = shader.parent().unwrap_or(Path::new(".")).to_path_buf();
            return Ok(Some(ResolvedCustomShader {
                working_dir,
                shader_name,
            }));
        }

        fs::create_dir_all(&context.session_paths.session_dir).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to create session shader directory {}: {}",
                    context.session_paths.session_dir.display(),
                    error
                ),
            )
        })?;

        let mut combined_shader = String::new();
        for shader_path in &plan.custom_shader_paths {
            let shader_source = fs::read_to_string(shader_path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "failed to read universal shader {}: {}",
                        shader_path.display(),
                        error
                    ),
                )
            })?;
            combined_shader.push_str(&shader_source);
            if !combined_shader.ends_with('\n') {
                combined_shader.push('\n');
            }
        }

        if plan.cas_strength > 0.0 {
            combined_shader.push_str(&render_cas_shader(plan.cas_strength));
        }

        let generated_shader_path = context
            .session_paths
            .session_dir
            .join(GENERATED_RUNTIME_SHADER_NAME);
        fs::write(&generated_shader_path, combined_shader).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to write generated universal shader {}: {}",
                    generated_shader_path.display(),
                    error
                ),
            )
        })?;

        Ok(Some(ResolvedCustomShader {
            working_dir: context.session_paths.session_dir.clone(),
            shader_name: GENERATED_RUNTIME_SHADER_NAME.to_string(),
        }))
    }

    fn derive_gop_length(&self, context: &StageRuntimeContext) -> u32 {
        let source_fps = context
            .source
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.source_fps);
        let output_fps =
            crate::pipeline::planned_target_frame_rate(source_fps, &context.execution_plan)
                .unwrap_or(60.0);
        (output_fps * self.context.profile.hls_time as f64).round() as u32
    }

    fn build_video_encode_args(
        &self,
        output_bit_depth: OutputBitDepth,
        gop_len: u32,
    ) -> Vec<String> {
        let mut args = vec![
            "-c:v".to_string(),
            self.context.encode_backend.encoder_name().to_string(),
            "-b:v".to_string(),
            format!("{}k", self.context.profile.bitrate),
            "-maxrate".to_string(),
            format!("{}k", self.context.profile.max_bitrate),
            "-bf".to_string(),
            self.context.profile.bframes.to_string(),
            "-g".to_string(),
            gop_len.to_string(),
        ];

        match self.context.encode_backend {
            UniversalEncodeBackend::Nvenc => {
                args.extend([
                    "-preset".to_string(),
                    self.context.profile.preset.clone(),
                    "-profile:v".to_string(),
                    ffmpeg_hevc_profile(output_bit_depth).to_string(),
                    "-rc".to_string(),
                    "vbr".to_string(),
                    "-rc-lookahead".to_string(),
                    self.context.profile.lookahead.to_string(),
                    "-spatial_aq".to_string(),
                    "1".to_string(),
                    "-temporal_aq".to_string(),
                    "1".to_string(),
                ]);
            }
            UniversalEncodeBackend::Vaapi => {
                args.extend([
                    "-profile:v".to_string(),
                    ffmpeg_hevc_profile(output_bit_depth).to_string(),
                    "-rc_mode".to_string(),
                    "VBR".to_string(),
                ]);
            }
            UniversalEncodeBackend::Amf => {
                // hevc_amf quality preset and VBR rate control.
                // -quality balanced keeps encode latency reasonable while
                // still achieving better quality than the speed preset.
                args.extend([
                    "-quality".to_string(),
                    "balanced".to_string(),
                    "-profile:v".to_string(),
                    ffmpeg_hevc_profile(output_bit_depth).to_string(),
                    "-rc".to_string(),
                    "vbr_latency".to_string(),
                ]);
            }
            UniversalEncodeBackend::CpuFallback => {
                args.extend([
                    "-preset".to_string(),
                    "medium".to_string(),
                    "-pix_fmt".to_string(),
                    self.context
                        .encode_backend
                        .pixel_format(output_bit_depth)
                        .to_string(),
                ]);
            }
        }

        args
    }

    fn build_output_args(
        &self,
        context: &StageRuntimeContext,
    ) -> io::Result<(Vec<String>, StdoutMode)> {
        match &context.transport.output {
            FrameTransport::HlsOutput {
                playlist_path,
                segment_dir,
            } => Ok((
                vec![
                    // HEVC in HLS requires the hvc1 codec tag for compatibility
                    // with Apple devices, Safari, and HLS players that validate
                    // the codec string. Without this, FFmpeg defaults to hev1.
                    "-tag:v".to_string(),
                    "hvc1".to_string(),
                    "-f".to_string(),
                    "hls".to_string(),
                    "-hls_time".to_string(),
                    self.context.profile.hls_time.to_string(),
                    "-hls_list_size".to_string(),
                    "8".to_string(),
                    "-hls_segment_type".to_string(),
                    "mpegts".to_string(),
                    "-hls_flags".to_string(),
                    "delete_segments".to_string(),
                    "-hls_segment_filename".to_string(),
                    segment_dir
                        .join("segment_%06d.ts")
                        .to_string_lossy()
                        .to_string(),
                    playlist_path.to_string_lossy().to_string(),
                ],
                StdoutMode::Null,
            )),
            FrameTransport::StdoutPipe => Ok((
                vec![
                    "-f".to_string(),
                    "mpegts".to_string(),
                    "-flush_packets".to_string(),
                    "1".to_string(),
                    "pipe:1".to_string(),
                ],
                StdoutMode::Transport(FrameTransport::StdoutPipe),
            )),
            FrameTransport::NamedPipe(path) => Ok((
                vec![
                    "-f".to_string(),
                    "mpegts".to_string(),
                    "-flush_packets".to_string(),
                    "1".to_string(),
                    path.to_string_lossy().to_string(),
                ],
                StdoutMode::Null,
            )),
            FrameTransport::LocalSocket(path) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "Universal FFmpeg executor does not yet support local-socket output at {:?}.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Universal FFmpeg executor cannot emit {:?} as its output transport.",
                    transport
                ),
            )),
        }
    }
}

impl BackendExecutor for UniversalExecutor {
    /// Builds the ExecutorSpec for this universal executor configured for a given runtime stage and startup attempt.
    ///
    /// The returned ExecutorSpec contains the process command, transports, stdin/stdout modes, and execution metadata
    /// required to spawn FFmpeg for the universal encoding path. If an executor-owned custom shader is present,
    /// the spec's process `current_dir` is set so FFmpeg can resolve filename-only `custom_shader_path` entries.
    /// The `stderr_mode` field is configured to signal NVENC-specific progress parsing when the selected backend is NVENC.
    ///
    /// # Examples
    ///
    fn build_executor_spec(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> io::Result<ExecutorSpec> {
        let startup_mode = LegacyStartupMode::for_attempt(attempt_number);
        let command = self.build_command(context, startup_mode)?;

        Ok(ExecutorSpec {
            kind: ExecutorKind::Universal,
            startup_label: Some(startup_mode.as_str().to_string()),
            process: ProcessSpec {
                stage: PipelineStageId::Executor,
                program: command.program,
                args: command.args,
                transport: context.transport.clone(),
                stdin: command.stdin,
                stdout: command.stdout,
                stderr_piped: true,
                current_dir: command.current_dir,
                env: Vec::new(),
                readiness_policy: PipelineStageReadinessPolicy::ReadyOnHeartbeat,
                log_label: self.context.encode_backend.log_label().to_string(),
                stderr_mode: StderrMode::FfmpegProgress {
                    gpu_vendor: match self.context.encode_backend {
                        UniversalEncodeBackend::Nvenc => Some("nvidia".to_string()),
                        UniversalEncodeBackend::Amf => Some("amd".to_string()),
                        _ => None,
                    },
                },
                kill_on_drop: true,
                hidden_window: true,
            },
        })
    }
}

fn build_ffmpeg_input_args(input: &EncoderInput, low_latency: bool) -> Vec<String> {
    match input {
        EncoderInput::SourceUrl { url, extra_headers } => {
            let mut args = vec![
                "-fflags".to_string(),
                if low_latency {
                    "+genpts+nobuffer".to_string()
                } else {
                    "+genpts".to_string()
                },
                "-reconnect".to_string(),
                "1".to_string(),
                "-reconnect_streamed".to_string(),
                "1".to_string(),
                "-reconnect_on_network_error".to_string(),
                "1".to_string(),
                "-reconnect_delay_max".to_string(),
                "5".to_string(),
                "-headers".to_string(),
                build_input_headers(extra_headers),
            ];

            if !low_latency {
                args.extend([
                    "-analyzeduration".to_string(),
                    INPUT_ANALYZE_MICROS.to_string(),
                    "-probesize".to_string(),
                    INPUT_PROBESIZE_BYTES.to_string(),
                ]);
            }

            args.extend(["-re".to_string(), "-i".to_string(), url.clone()]);
            args
        }
        EncoderInput::StdinPipe => vec![
            "-fflags".to_string(),
            "+discardcorrupt+nobuffer".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            "-i".to_string(),
            "pipe:0".to_string(),
        ],
        EncoderInput::NamedPipe(path) => vec![
            "-fflags".to_string(),
            "+discardcorrupt+nobuffer".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            "-i".to_string(),
            path.to_string_lossy().to_string(),
        ],
    }
}

fn build_input_headers(extra_headers: &HashMap<String, String>) -> String {
    let mut header_str = format!("User-Agent: {}\r\n", REQUEST_USER_AGENT);
    for (key, value) in extra_headers {
        header_str.push_str(&format!("{}: {}\r\n", key, value));
    }
    header_str
}

fn build_libplacebo_filter(
    plan: &UniversalFilterPlan,
    custom_shader: Option<&ResolvedCustomShader>,
) -> Option<String> {
    if !plan.has_libplacebo_processing() {
        return None;
    }

    let mut args = Vec::new();

    if let Some((width, height)) = plan.output_resolution.as_deref().and_then(parse_resolution) {
        if plan.custom_shader_paths.is_empty() {
            push_aspect_preserving_resize_args(
                &mut args,
                width,
                height,
                map_quality_to_libplacebo_scaler(plan.resize_quality),
            );
        } else {
            // When a custom upscale shader is active, let the shader handle
            // spatial reconstruction instead of libplacebo's built-in scaler.
            push_aspect_preserving_resize_args(&mut args, width, height, "none");
        }
    }

    if let Some(shader) = custom_shader {
        args.push(format!("custom_shader_path={}", shader.shader_name));
    }

    if plan.tone_map_to_hdr10 {
        args.push("tonemapping=bt.2390".to_string());
    }
    if let Some(color_primaries) = map_color_primaries(plan.color_primaries) {
        args.push(format!("color_primaries={}", color_primaries));
    }
    if let Some(color_trc) = map_transfer(plan.transfer) {
        args.push(format!("color_trc={}", color_trc));
    }
    if let Some(colorspace) = map_matrix(plan.matrix) {
        args.push(format!("colorspace={}", colorspace));
    }
    args.push(format!(
        "format={}",
        if plan.output_bit_depth == OutputBitDepth::Bit10 {
            "yuv420p10le"
        } else {
            "yuv420p"
        }
    ));

    Some(format!("libplacebo={}", args.join(":")))
}

use super::libplacebo_filters::{
    map_color_primaries, map_matrix, map_quality_to_libplacebo_scaler, map_transfer,
    parse_resolution, push_aspect_preserving_resize_args,
};

fn ffmpeg_hevc_profile(output_bit_depth: OutputBitDepth) -> &'static str {
    if output_bit_depth == OutputBitDepth::Bit10 {
        "main10"
    } else {
        "main"
    }
}

fn render_cas_shader(cas_strength: f32) -> String {
    CAS_SHADER_TEMPLATE.replace(
        CAS_SHADER_STRENGTH_PLACEHOLDER,
        &format!("{:.6}", cas_strength.clamp(0.0, 1.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        select_universal_encode_backend, ResolvedCustomShader, RuntimePlatform,
        UniversalBackendSelectionContext, UniversalEncodeBackend, UniversalExecutor,
        UniversalExecutorContext, UniversalFilterPlan, GENERATED_RUNTIME_SHADER_NAME,
    };
    use crate::graph::{
        Artcnn2xPlan, ColorPrimariesIntent, ExecutionPlan, ExecutorKind, HdrMetadataKind,
        HdrMetadataPlan, HdrPlan, HdrRequest, HdrTransformKind, HdrTransformPlan,
        InterpolationDecision, InterpolationPlan, LatencyMode, MatrixCoefficientsIntent,
        OpExecutionStage, OutputBitDepth, ResizePlan, TransferCharacteristicIntent, VideoOp,
    };
    use crate::pipeline::EncodingProfile;
    use crate::runtime::{
        BackendExecutor, FrameTransport, LegacyStartupMode, SessionOutputPaths,
        StageRuntimeContext, StdinMode, StdoutMode, TransportConfig,
    };
    use crate::source::{SourceClassification, SourceDescriptor, SourceKind, SourceTransport};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// Constructs a StageRuntimeContext configured for the Universal executor using the given video operations and transports.
    ///
    /// This helper is intended for tests and creates a minimal, valid context with:
    /// - `executor` set to `ExecutorKind::Universal`
    /// - a balanced latency mode and a `requires_local_hls_relay` flag set to `true`
    /// - example source URLs and empty headers/metadata
    ///
    /// # Examples
    ///
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

    fn temp_session_context(
        video_ops: Vec<VideoOp>,
        input: FrameTransport,
        output: FrameTransport,
    ) -> StageRuntimeContext {
        let mut stage_context = context(video_ops, input, output);
        let session_dir =
            std::env::temp_dir().join(format!("referee-universal-{}", Uuid::new_v4()));
        let playlist_path = session_dir.join("index.m3u8");
        stage_context.session_paths = SessionOutputPaths {
            session_dir: session_dir.clone(),
            packager_playlist_path: playlist_path.clone(),
        };

        if matches!(
            stage_context.transport.output,
            FrameTransport::HlsOutput { .. }
        ) {
            stage_context.transport.output = FrameTransport::HlsOutput {
                playlist_path,
                segment_dir: session_dir,
            };
        }

        stage_context
    }

    /// Creates a UniversalExecutorContext with a sensible default encoding profile and the provided backend/shader configuration.
    ///
    /// The returned context uses a default EncodingProfile (25,000 kbps bitrate, 37,500 kbps max bitrate, preset `"p4"`, lookahead 8, 3 B-frames, 1s HLS segment),
    /// resolves `intermediate_plan` from `stage_context.execution_plan`, and sets a default VAAPI device of `/dev/dri/renderD128` when applicable.
    ///
    /// # Examples
    ///
    fn universal_executor_context(
        encode_backend: UniversalEncodeBackend,
        stage_context: &StageRuntimeContext,
        anime4k_shaders_dir: Option<PathBuf>,
        artcnn_shaders_dir: Option<PathBuf>,
    ) -> UniversalExecutorContext {
        UniversalExecutorContext {
            profile: EncodingProfile {
                bitrate: 25_000,
                max_bitrate: 37_500,
                preset: "p4".to_string(),
                lookahead: 8,
                bframes: 3,
                hls_time: 1,
            },
            ffmpeg_program: PathBuf::from("ffmpeg"),
            intermediate_plan: stage_context.execution_plan.to_intermediate_plan(),
            encode_backend,
            vaapi_device: Some(PathBuf::from("/dev/dri/renderD128")),
            anime4k_shaders_dir,
            artcnn_shaders_dir,
            denoise: false,
            cas_strength: 0.0,
        }
    }
    ///
    /// # Examples
    ///
    fn universal_executor(
        encode_backend: UniversalEncodeBackend,
        stage_context: &StageRuntimeContext,
    ) -> UniversalExecutor {
        UniversalExecutor::new(universal_executor_context(
            encode_backend,
            stage_context,
            None,
            None,
        ))
    }

    /// Extracts the filter graph string provided to the `-vf` FFmpeg argument from an argument slice.
    ///
    /// # Returns
    ///
    /// `Some(&str)` containing the filter expression that follows `-vf`, or `None` if `-vf` is not present.
    ///
    /// # Examples
    ///
    fn filter_value(args: &[String]) -> Option<&str> {
        args.windows(2)
            .find(|window| window[0] == "-vf")
            .map(|window| window[1].as_str())
    }

    fn has_subsequence(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }

    #[test]
    fn filter_plan_maps_resize_intent_to_libplacebo_resize_args() {
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Resize(ResizePlan {
                target_resolution: "3840x2160".to_string(),
                quality: Some(3),
                stage: OpExecutionStage::Executor,
            })],
            None,
            None,
        );

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("resize filter graph");

        assert_eq!(plan.output_resolution.as_deref(), Some("3840x2160"));
        assert_eq!(plan.resize_quality, Some(3));
        assert!(filter_graph.contains("libplacebo="));
        assert!(filter_graph.contains("w=3840"));
        assert!(filter_graph.contains("h=2160"));
        assert!(filter_graph.contains("force_original_aspect_ratio=decrease"));
        assert!(filter_graph.contains("normalize_sar=true"));
        assert!(filter_graph.contains("pad_crop_ratio=0.0"));
        assert!(filter_graph.contains("upscaler=ewa_lanczos"));
    }

    #[test]
    fn filter_plan_can_prepend_hqdn3d_without_libplacebo() {
        let mut plan = UniversalFilterPlan::default();
        plan.denoise = true;

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("denoise filter graph");

        assert_eq!(filter_graph, "hqdn3d,format=nv12");
    }

    #[test]
    fn filter_plan_maps_hdr_intent_to_libplacebo_hdr_args() {
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Hdr(HdrPlan {
                request: HdrRequest::TonemapToHdr10,
                output_bit_depth: OutputBitDepth::Bit10,
                color_primaries: ColorPrimariesIntent::Bt2020,
                transfer: TransferCharacteristicIntent::Smpte2084,
                matrix: MatrixCoefficientsIntent::Bt2020Nc,
                metadata: Some(HdrMetadataPlan {
                    kind: HdrMetadataKind::Hdr10Static,
                    stage: OpExecutionStage::Executor,
                }),
                transform: Some(HdrTransformPlan {
                    kind: HdrTransformKind::TonemapToHdr10,
                    stage: OpExecutionStage::Executor,
                }),
            })],
            None,
            None,
        );

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("hdr filter graph");

        assert_eq!(plan.output_bit_depth, OutputBitDepth::Bit10);
        assert!(plan.tone_map_to_hdr10);
        assert!(filter_graph.contains("tonemapping=bt.2390"));
        assert!(filter_graph.contains("color_primaries=bt2020"));
        assert!(filter_graph.contains("color_trc=smpte2084"));
        assert!(filter_graph.contains("colorspace=bt2020nc"));
        assert!(filter_graph.contains("format=yuv420p10le"));
    }

    #[test]
    fn filter_plan_skips_resize_when_preprocess_stage_owns_it() {
        // When the planner assigns resize to the Preprocess stage the
        // FfmpegPreprocessor renders the libplacebo scale filter.
        // The Universal executor must not duplicate that work.
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Resize(ResizePlan {
                target_resolution: "3840x2160".to_string(),
                quality: Some(2),
                stage: OpExecutionStage::Preprocess,
            })],
            None,
            None,
        );

        assert!(
            plan.output_resolution.is_none(),
            "executor must not own resize when stage is Preprocess"
        );
        assert!(plan.resize_quality.is_none());
        assert!(!plan.has_libplacebo_processing());
    }

    #[test]
    fn filter_plan_skips_hdr_color_transform_when_preprocess_owns_it_but_preserves_bit_depth() {
        // When the HDR transform stage is Preprocess, color/tone processing
        // belongs to the preprocessor.  The Universal executor must not apply
        // a libplacebo tonemap filter, but it still needs the output bit depth
        // to select the correct encoder profile (main10 vs main).
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Hdr(HdrPlan {
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
            })],
            None,
            None,
        );

        // Bit depth preserved — encoder needs main10 profile.
        assert_eq!(plan.output_bit_depth, OutputBitDepth::Bit10);
        // Color/tone fields reset — no libplacebo HDR filter in the executor.
        assert!(!plan.tone_map_to_hdr10);
        assert!(matches!(
            plan.color_primaries,
            ColorPrimariesIntent::PreserveSource
        ));
        assert!(matches!(
            plan.transfer,
            TransferCharacteristicIntent::PreserveSource
        ));
        assert!(matches!(
            plan.matrix,
            MatrixCoefficientsIntent::PreserveSource
        ));
        // has_libplacebo_processing is now false: the preprocess stage already
        // delivered yuv420p10le frames.  The executor skips libplacebo entirely
        // and relies on the backend-tail format conversion (e.g. `format=p010le`
        // for NVENC) to satisfy the encoder's pixel-format expectation.
        assert!(!plan.has_libplacebo_processing());
    }

    #[test]
    fn filter_plan_no_double_render_when_both_resize_and_hdr_are_preprocess_owned() {
        // Full SharedPreprocess path: both resize and HDR are at Preprocess
        // stage.  The executor filter plan must contain no resize dimensions,
        // no tonemap/color processing, and no executor-side libplacebo filter.
        // The preprocess stage already delivered frames in the correct pixel
        // format; the backend tail (e.g. `format=p010le` for NVENC) handles
        // the explicit pixel-format handshake the encoder needs.
        let plan = UniversalFilterPlan::from_video_ops(
            &[
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
            None,
            None,
        );

        assert!(plan.output_resolution.is_none(), "no resize in executor");
        assert!(!plan.tone_map_to_hdr10, "no tonemap in executor");
        assert!(
            matches!(plan.color_primaries, ColorPrimariesIntent::PreserveSource),
            "no color primaries override in executor"
        );
        // Bit depth is still known so the encoder can select main10.
        assert_eq!(plan.output_bit_depth, OutputBitDepth::Bit10);

        // No libplacebo filter at all: preprocess already delivered the frames
        // in the correct pixel format.  The backend tail handles the explicit
        // format conversion required by the encoder (e.g. `format=p010le` for
        // NVENC so that hevc_nvenc receives the semi-planar 10-bit format it
        // expects, rather than relying on FFmpeg's implicit auto-conversion).
        assert!(
            !plan.has_libplacebo_processing(),
            "no executor-side libplacebo work when both resize and HDR are preprocess-owned"
        );
        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("backend format tail still emitted for 10-bit output");
        assert!(
            filter_graph.contains("format=p010le"),
            "NVENC backend tail sets explicit p010le format: {}",
            filter_graph
        );
        assert!(
            !filter_graph.contains("libplacebo="),
            "no libplacebo invocation in executor filter: {}",
            filter_graph
        );
        assert!(
            !filter_graph.contains("w=") && !filter_graph.contains("h="),
            "no resize geometry in executor filter: {}",
            filter_graph
        );
        assert!(
            !filter_graph.contains("tonemapping="),
            "no tone-map in executor filter: {}",
            filter_graph
        );
        assert!(
            !filter_graph.contains("color_primaries="),
            "no color primaries override in executor filter: {}",
            filter_graph
        );
    }

    #[test]
    fn filter_plan_still_applies_executor_stage_hdr_passthrough_10bit() {
        // When Passthrough10Bit is at Executor stage (TemporaryExecutorFallback)
        // the Universal executor must still apply the pixel format conversion.
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Hdr(HdrPlan {
                request: HdrRequest::Passthrough10Bit,
                output_bit_depth: OutputBitDepth::Bit10,
                color_primaries: ColorPrimariesIntent::PreserveSource,
                transfer: TransferCharacteristicIntent::PreserveSource,
                matrix: MatrixCoefficientsIntent::PreserveSource,
                metadata: None,
                transform: Some(HdrTransformPlan {
                    kind: HdrTransformKind::Passthrough10Bit,
                    stage: OpExecutionStage::Executor,
                }),
            })],
            None,
            None,
        );

        assert_eq!(plan.output_bit_depth, OutputBitDepth::Bit10);
        // Passthrough10Bit does not tonemap, it just promotes pixel format.
        assert!(!plan.tone_map_to_hdr10);
        // has_libplacebo_processing is true because bit depth is 10-bit.
        assert!(plan.has_libplacebo_processing());
    }

    #[test]
    fn filter_plan_keeps_interpolation_deferred_without_emitting_framegen_filters() {
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision: InterpolationDecision::portable_fallback(OpExecutionStage::Executor),
            })],
            None,
            None,
        );

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("backend format tail");

        assert!(plan.deferred_interpolation);
        assert!(!plan.has_libplacebo_processing());
        assert!(!filter_graph.contains("minterpolate"));
        assert!(!filter_graph.contains("fps=60000/1001"));
    }

    #[test]
    fn filter_plan_does_not_mark_interpolation_deferred_when_preprocess_owns_it() {
        // portable_fallback(Preprocess) with no gap reason means
        // FfmpegPreprocessor handles the minterpolate filter.
        // The Universal executor must not mark it as deferred.
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision: InterpolationDecision::portable_fallback(OpExecutionStage::Preprocess),
            })],
            None,
            None,
        );

        assert!(!plan.deferred_interpolation);
        assert!(!plan.has_libplacebo_processing());
    }

    #[test]
    fn backend_selection_prefers_amf_for_windows_amd() {
        assert_eq!(
            select_universal_encode_backend(UniversalBackendSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "amd",
            }),
            UniversalEncodeBackend::Amf
        );
    }

    #[test]
    fn backend_selection_prefers_nvenc_for_linux_nvidia() {
        assert_eq!(
            select_universal_encode_backend(UniversalBackendSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "nvidia",
            }),
            UniversalEncodeBackend::Nvenc
        );
    }

    #[test]
    fn backend_selection_prefers_vaapi_for_linux_amd() {
        assert_eq!(
            select_universal_encode_backend(UniversalBackendSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "amd",
            }),
            UniversalEncodeBackend::Vaapi
        );
    }

    #[test]
    fn backend_selection_uses_cpu_fallback_for_unknown_vendor() {
        assert_eq!(
            select_universal_encode_backend(UniversalBackendSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "unknown",
            }),
            UniversalEncodeBackend::CpuFallback
        );
    }

    #[test]
    fn linux_backend_selection_wires_to_expected_ffmpeg_hardware_encoders() {
        let nvidia_backend = select_universal_encode_backend(UniversalBackendSelectionContext {
            platform: RuntimePlatform::Linux,
            gpu_vendor: "nvidia",
        });
        let amd_backend = select_universal_encode_backend(UniversalBackendSelectionContext {
            platform: RuntimePlatform::Linux,
            gpu_vendor: "amd",
        });

        let nvidia_context = context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let amd_context = context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let nvidia_command = universal_executor(nvidia_backend, &nvidia_context)
            .build_command(&nvidia_context, LegacyStartupMode::LowLatency)
            .unwrap();
        let amd_command = universal_executor(amd_backend, &amd_context)
            .build_command(&amd_context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert_eq!(nvidia_backend, UniversalEncodeBackend::Nvenc);
        assert!(has_subsequence(
            &nvidia_command.args,
            &["-c:v", "hevc_nvenc"]
        ));
        assert_eq!(amd_backend, UniversalEncodeBackend::Vaapi);
        assert!(has_subsequence(&amd_command.args, &["-c:v", "hevc_vaapi"]));
        assert!(has_subsequence(
            &amd_command.args,
            &["-vaapi_device", "/dev/dri/renderD128"]
        ));
    }

    #[test]
    fn build_command_uses_cpu_fallback_encoder_for_unknown_vendor_selection() {
        let stage_context = context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let executor = universal_executor(
            select_universal_encode_backend(UniversalBackendSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "unknown",
            }),
            &stage_context,
        );
        let command = executor
            .build_command(&stage_context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(has_subsequence(&command.args, &["-c:v", "libx265"]));
        assert!(has_subsequence(&command.args, &["-pix_fmt", "yuv420p"]));
    }

    #[test]
    fn build_command_uses_libplacebo_resize_and_hdr_with_nvenc_encode() {
        let stage_context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(3),
                    stage: OpExecutionStage::Executor,
                }),
                VideoOp::Hdr(HdrPlan {
                    request: HdrRequest::TonemapToHdr10,
                    output_bit_depth: OutputBitDepth::Bit10,
                    color_primaries: ColorPrimariesIntent::Bt2020,
                    transfer: TransferCharacteristicIntent::Smpte2084,
                    matrix: MatrixCoefficientsIntent::Bt2020Nc,
                    metadata: Some(HdrMetadataPlan {
                        kind: HdrMetadataKind::Hdr10Static,
                        stage: OpExecutionStage::Executor,
                    }),
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::TonemapToHdr10,
                        stage: OpExecutionStage::Executor,
                    }),
                }),
            ],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        let executor = universal_executor(UniversalEncodeBackend::Nvenc, &stage_context);
        let command = executor
            .build_command(&stage_context, LegacyStartupMode::Buffered)
            .unwrap();

        let filter = filter_value(&command.args).expect("filter graph");
        assert_eq!(command.stdin, StdinMode::Null);
        assert_eq!(command.stdout, StdoutMode::Null);
        assert!(filter.contains("libplacebo="));
        assert!(filter.contains("w=3840"));
        assert!(filter.contains("h=2160"));
        assert!(filter.contains("force_original_aspect_ratio=decrease"));
        assert!(filter.contains("normalize_sar=true"));
        assert!(filter.contains("pad_crop_ratio=0.0"));
        assert!(filter.contains("upscaler=ewa_lanczos"));
        assert!(filter.contains("tonemapping=bt.2390"));
        assert!(filter.contains("color_primaries=bt2020"));
        assert!(filter.contains("color_trc=smpte2084"));
        assert!(filter.contains("colorspace=bt2020nc"));
        assert!(has_subsequence(&command.args, &["-c:v", "hevc_nvenc"]));
        assert!(has_subsequence(&command.args, &["-profile:v", "main10"]));
        assert!(has_subsequence(&command.args, &["-tag:v", "hvc1"]));
        assert!(has_subsequence(&command.args, &["-f", "hls"]));
    }

    #[test]
    fn build_command_uses_vaapi_upload_chain_for_linux_amd_path() {
        let stage_context = context(
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
        let executor = universal_executor(UniversalEncodeBackend::Vaapi, &stage_context);
        let command = executor
            .build_command(&stage_context, LegacyStartupMode::LowLatency)
            .unwrap();

        let filter = filter_value(&command.args).expect("filter graph");
        assert_eq!(
            command.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(
            command.stdout,
            StdoutMode::Transport(FrameTransport::StdoutPipe)
        );
        assert!(filter.contains("libplacebo="));
        assert!(filter.contains("format=nv12,hwupload"));
        assert!(has_subsequence(
            &command.args,
            &["-vaapi_device", "/dev/dri/renderD128"]
        ));
        assert!(has_subsequence(&command.args, &["-c:v", "hevc_vaapi"]));
        assert!(has_subsequence(
            &command.args,
            &["-f", "mpegts", "-flush_packets", "1"]
        ));
        assert_eq!(command.args.last().map(String::as_str), Some("pipe:1"));
    }

    #[test]
    fn build_command_does_not_fake_interpolation_support() {
        let stage_context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback(OpExecutionStage::Executor),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let executor = universal_executor(UniversalEncodeBackend::Nvenc, &stage_context);
        let command = executor
            .build_command(&stage_context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(!command.args.iter().any(|arg| arg.contains("minterpolate")));
        assert!(!command
            .args
            .iter()
            .any(|arg| arg.contains("fps=60000/1001")));
    }

    #[test]
    fn build_command_routes_cas_only_through_generated_libplacebo_shader() {
        let stage_context = temp_session_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let mut executor_context =
            universal_executor_context(UniversalEncodeBackend::Nvenc, &stage_context, None, None);
        executor_context.cas_strength = 0.5;
        let executor = UniversalExecutor::new(executor_context);
        let command = executor
            .build_command(&stage_context, LegacyStartupMode::LowLatency)
            .unwrap();

        let filter = filter_value(&command.args).expect("cas filter graph");
        let generated_shader = stage_context
            .session_paths
            .session_dir
            .join(GENERATED_RUNTIME_SHADER_NAME);
        let shader_source = std::fs::read_to_string(&generated_shader).unwrap();

        assert!(filter.contains("libplacebo="));
        assert!(filter.contains("custom_shader_path=referee-universal-runtime.glsl"));
        assert!(!filter.contains("cas=strength="));
        assert!(has_subsequence(
            &command.args,
            &["-init_hw_device", "vulkan=vk:0"]
        ));
        assert!(has_subsequence(&command.args, &["-filter_hw_device", "vk"]));
        assert_eq!(
            command.current_dir,
            Some(stage_context.session_paths.session_dir.clone())
        );
        assert!(shader_source.contains("#define REFEREE_CAS_STRENGTH 0.500000"));
        assert!(shader_source.contains("//!DESC Referee-CAS-Sharpen"));

        let _ = std::fs::remove_dir_all(&stage_context.session_paths.session_dir);
    }

    #[test]
    fn build_command_applies_hqdn3d_only_on_direct_source_pull_when_enabled() {
        let direct_context = temp_session_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::SourcePull,
            FrameTransport::StdoutPipe,
        );
        let mut direct_executor_context =
            universal_executor_context(UniversalEncodeBackend::Nvenc, &direct_context, None, None);
        direct_executor_context.denoise = true;
        let direct_executor = UniversalExecutor::new(direct_executor_context);
        let direct_command = direct_executor
            .build_command(&direct_context, LegacyStartupMode::LowLatency)
            .unwrap();
        let direct_filter = filter_value(&direct_command.args).expect("direct filter graph");

        assert!(direct_filter.contains("hqdn3d"));

        let staged_context = temp_session_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let mut staged_executor_context =
            universal_executor_context(UniversalEncodeBackend::Nvenc, &staged_context, None, None);
        staged_executor_context.denoise = true;
        let staged_executor = UniversalExecutor::new(staged_executor_context);
        let staged_command = staged_executor
            .build_command(&staged_context, LegacyStartupMode::LowLatency)
            .unwrap();
        let staged_filter = filter_value(&staged_command.args).expect("staged filter graph");

        assert!(
            !staged_filter.contains("hqdn3d"),
            "staged inputs should rely on the FFmpeg normalizer for denoise: {}",
            staged_filter
        );
    }

    #[test]
    fn filter_plan_maps_artcnn_intent_to_custom_shader_path() {
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                target_resolution: "3840x2160".to_string(),
                stage: OpExecutionStage::Executor,
            })],
            None,
            Some(std::path::Path::new("lib\\shaders\\artcnn")),
        );

        let filter_graph = plan
            .build_filter_graph(
                UniversalEncodeBackend::Nvenc,
                Some(&ResolvedCustomShader {
                    working_dir: PathBuf::from("lib\\shaders\\artcnn"),
                    shader_name: "ArtCNN_C4F16.glsl".to_string(),
                }),
            )
            .expect("artcnn filter graph");

        assert_eq!(plan.output_resolution.as_deref(), Some("3840x2160"));
        assert!(filter_graph.contains("custom_shader_path=ArtCNN_C4F16.glsl"));
        assert!(filter_graph.contains("force_original_aspect_ratio=decrease"));
        assert!(filter_graph.contains("normalize_sar=true"));
        assert!(filter_graph.contains("pad_crop_ratio=0.0"));
        assert!(filter_graph.contains("upscaler=none"));
    }

    #[test]
    fn build_command_combines_executor_shader_and_cas_in_session_shader() {
        let stage_context = temp_session_context(
            vec![VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                target_resolution: "3840x2160".to_string(),
                stage: OpExecutionStage::Executor,
            })],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let shader_dir =
            std::env::temp_dir().join(format!("referee-universal-shaders-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&shader_dir).unwrap();
        std::fs::write(
            shader_dir.join("ArtCNN_C4F16.glsl"),
            "//!DESC Test-ArtCNN\n//!HOOK MAIN\n//!BIND MAIN\nvec4 hook(){ return MAIN_texOff(vec2(0.0, 0.0)); }\n",
        )
        .unwrap();

        let mut executor_context = universal_executor_context(
            UniversalEncodeBackend::Nvenc,
            &stage_context,
            None,
            Some(shader_dir.clone()),
        );
        executor_context.cas_strength = 0.5;
        let executor = UniversalExecutor::new(executor_context);
        let command = executor
            .build_command(&stage_context, LegacyStartupMode::LowLatency)
            .unwrap();
        let filter = filter_value(&command.args).expect("combined shader filter graph");
        let generated_shader = stage_context
            .session_paths
            .session_dir
            .join(GENERATED_RUNTIME_SHADER_NAME);
        let shader_source = std::fs::read_to_string(&generated_shader).unwrap();

        assert!(filter.contains("custom_shader_path=referee-universal-runtime.glsl"));
        assert!(filter.contains("force_original_aspect_ratio=decrease"));
        assert!(filter.contains("normalize_sar=true"));
        assert!(filter.contains("pad_crop_ratio=0.0"));
        assert!(filter.contains("upscaler=none"));
        assert_eq!(
            command.current_dir,
            Some(stage_context.session_paths.session_dir.clone())
        );
        assert!(shader_source.contains("Test-ArtCNN"));
        assert!(shader_source.contains("#define REFEREE_CAS_STRENGTH 0.500000"));

        let _ = std::fs::remove_dir_all(&stage_context.session_paths.session_dir);
        let _ = std::fs::remove_dir_all(shader_dir);
    }

    #[test]
    fn filter_plan_skips_shader_upscale_when_preprocess_owns_it() {
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                target_resolution: "3840x2160".to_string(),
                stage: OpExecutionStage::Preprocess,
            })],
            None,
            Some(std::path::Path::new("lib\\shaders\\artcnn")),
        );

        assert!(plan.output_resolution.is_none());
        assert!(plan.custom_shader_paths.is_empty());
        assert!(!plan.has_libplacebo_processing());
    }

    #[test]
    fn build_executor_spec_uses_shader_working_directory_only_for_executor_owned_shader() {
        let executor_owned_context = context(
            vec![VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                target_resolution: "3840x2160".to_string(),
                stage: OpExecutionStage::Executor,
            })],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let executor_owned = UniversalExecutor::new(universal_executor_context(
            UniversalEncodeBackend::Nvenc,
            &executor_owned_context,
            None,
            Some(PathBuf::from("/tmp/shaders/artcnn")),
        ));
        let executor_owned_spec = executor_owned
            .build_executor_spec(&executor_owned_context, 1)
            .unwrap();

        assert_eq!(
            executor_owned_spec.process.current_dir,
            Some(PathBuf::from("/tmp/shaders/artcnn"))
        );

        let preprocess_owned_context = context(
            vec![VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                target_resolution: "3840x2160".to_string(),
                stage: OpExecutionStage::Preprocess,
            })],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let preprocess_owned = UniversalExecutor::new(universal_executor_context(
            UniversalEncodeBackend::Nvenc,
            &preprocess_owned_context,
            None,
            Some(PathBuf::from("/tmp/shaders/artcnn")),
        ));
        let preprocess_owned_spec = preprocess_owned
            .build_executor_spec(&preprocess_owned_context, 1)
            .unwrap();

        assert_eq!(preprocess_owned_spec.process.current_dir, None);
    }

    #[test]
    fn preprocess_owned_hdr_emits_no_libplacebo_and_explicit_p010le_tail_for_nvenc() {
        // When the HDR transform is preprocess-owned the executor must not fire
        // a libplacebo filter.  For NVENC the backend tail must explicitly set
        // `format=p010le` so the encoder receives the semi-planar 10-bit layout
        // it expects, rather than auto-converting from yuv420p10le implicitly.
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Hdr(HdrPlan {
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
            })],
            None,
            None,
        );

        assert_eq!(plan.output_bit_depth, OutputBitDepth::Bit10);
        assert!(plan.hdr_preprocess_owned);
        assert!(!plan.has_libplacebo_processing());

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("backend tail still emitted for 10-bit output");

        assert!(
            !filter_graph.contains("libplacebo="),
            "no libplacebo filter for preprocess-owned HDR: {}",
            filter_graph
        );
        assert!(
            filter_graph.contains("format=p010le"),
            "explicit p010le format tail for NVENC: {}",
            filter_graph
        );
    }

    #[test]
    fn preprocess_owned_hdr_emits_no_libplacebo_and_explicit_format_tail_for_cpu_fallback() {
        // CPU fallback receives yuv420p10le from the preprocess stage.
        // No libplacebo filter should run; the backend tail sets the format
        // explicitly so the libx265 encoder gets the correct pixel format.
        let plan = UniversalFilterPlan::from_video_ops(
            &[VideoOp::Hdr(HdrPlan {
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
            })],
            None,
            None,
        );

        assert!(!plan.has_libplacebo_processing());

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::CpuFallback, None)
            .expect("backend tail still emitted for 10-bit output");

        assert!(
            !filter_graph.contains("libplacebo="),
            "no libplacebo filter for preprocess-owned HDR: {}",
            filter_graph
        );
        assert!(
            filter_graph.contains("format=yuv420p10le"),
            "explicit yuv420p10le format tail for CPU fallback: {}",
            filter_graph
        );
    }

    #[test]
    fn executor_side_resize_still_fires_libplacebo_when_hdr_is_preprocess_owned() {
        // Mixed case: resize is executor-owned (TemporaryExecutorFallback),
        // HDR transform is preprocess-owned.  The executor must still run
        // libplacebo for the resize; it must NOT add tonemap or colour args.
        let plan = UniversalFilterPlan::from_video_ops(
            &[
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
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
                        stage: OpExecutionStage::Preprocess,
                    }),
                }),
            ],
            None,
            None,
        );

        assert_eq!(plan.output_resolution.as_deref(), Some("3840x2160"));
        assert!(plan.hdr_preprocess_owned);
        // Resize causes libplacebo processing even though HDR is preprocess-owned.
        assert!(plan.has_libplacebo_processing());

        let filter_graph = plan
            .build_filter_graph(UniversalEncodeBackend::Nvenc, None)
            .expect("resize filter graph");

        assert!(
            filter_graph.contains("libplacebo="),
            "libplacebo present for executor-side resize: {}",
            filter_graph
        );
        assert!(
            filter_graph.contains("w=3840") && filter_graph.contains("h=2160"),
            "resize geometry present: {}",
            filter_graph
        );
        assert!(
            !filter_graph.contains("tonemapping="),
            "no tonemap in executor filter (preprocess owns it): {}",
            filter_graph
        );
        assert!(
            !filter_graph.contains("color_primaries="),
            "no colour transform in executor filter (preprocess owns it): {}",
            filter_graph
        );
    }

    #[test]
    fn build_executor_spec_keeps_universal_stage_identity() {
        let stage_context = context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let executor = universal_executor(UniversalEncodeBackend::CpuFallback, &stage_context);
        let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::Universal);
        assert_eq!(
            spec.process.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(
            spec.process.stdout,
            StdoutMode::Transport(FrameTransport::StdoutPipe)
        );
    }
}
