use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use crate::graph::{
    ExecutorKind, HdrPlan, IntermediateExecutionPlan, IntermediateOpOwner, IntermediateOperation,
    NativeAcceleratorKind, OutputBitDepth,
};
use crate::pipeline::{EncoderCapabilities, EncodingProfile};
use crate::runtime::{
    BackendExecutor, ExecutorSpec, FrameTransport, LegacyStartupMode, PipelineStageId,
    PipelineStageReadinessPolicy, ProcessSpec, StageRuntimeContext, StderrMode, StdinMode,
    StdoutMode,
};

use super::{
    append_rigaya_denoise_args, build_rigaya_input_args, resolve_encoder_input, EncoderInput,
};

/// Premium backend-native executor for the specialized Windows + NVIDIA path.
///
/// This path intentionally stays backed by NVEncC / Rigaya rather than FFmpeg
/// so it can continue to realize Windows + NVIDIA-specific features such as
/// NGX-VSR, FRUC, and TrueHDR directly through the native backend contract.
#[derive(Debug, Clone)]
pub struct NvidiaSpecializedExecutorContext {
    pub profile: EncodingProfile,
    pub encoder_path: PathBuf,
    #[allow(dead_code)]
    pub capabilities: EncoderCapabilities,
    pub intermediate_plan: IntermediateExecutionPlan,
    /// Apply GPU-side denoising equivalent to FFmpeg `hqdn3d` when this
    /// executor reads from the source directly rather than a staged normalizer.
    pub denoise: bool,
    /// CAS sharpening strength applied via `--vpp-unsharp`. Range [0.0, 1.0]; 0.0 disables.
    pub cas_strength: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NvidiaSpecializedExecutorCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: StdinMode,
    pub stdout: StdoutMode,
}

pub struct NvidiaSpecializedExecutor {
    context: NvidiaSpecializedExecutorContext,
}

const INLINE_HLS_LIST_SIZE: u32 = 8;
const INLINE_HLS_DELETE_THRESHOLD: u32 = INLINE_HLS_LIST_SIZE;
const UNKNOWN_SOURCE_FPS_FALLBACK: f64 = 30.0;
const UNKNOWN_FRUC_OUTPUT_FPS_FALLBACK: f64 = 60.0;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlannedOutputResolution {
    ExecutorResize(String),
    CompatibilityFallback(String),
}

impl PlannedOutputResolution {
    fn as_str(&self) -> &str {
        match self {
            Self::ExecutorResize(resolution) | Self::CompatibilityFallback(resolution) => {
                resolution.as_str()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PlannedNvencOptions {
    output_resolution: Option<PlannedOutputResolution>,
    resize_filter: Option<String>,
    hdr_args: Vec<String>,
    enable_fruc: bool,
    profile: &'static str,
    cas_strength: f32,
}

impl Default for PlannedNvencOptions {
    fn default() -> Self {
        Self {
            output_resolution: None,
            resize_filter: None,
            hdr_args: Vec::new(),
            enable_fruc: false,
            profile: "main",
            cas_strength: 0.0,
        }
    }
}

impl NvidiaSpecializedExecutor {
    pub fn new(context: NvidiaSpecializedExecutorContext) -> Self {
        Self { context }
    }

    fn should_prefer_buffered_source_ingest(context: &StageRuntimeContext) -> bool {
        matches!(context.transport.input, FrameTransport::SourcePull)
            && context.source.classification.is_hls_like()
    }

    fn should_force_cfr_for_direct_source(context: &StageRuntimeContext) -> bool {
        matches!(context.transport.input, FrameTransport::SourcePull)
    }

    fn effective_startup_mode(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> LegacyStartupMode {
        if Self::should_prefer_buffered_source_ingest(context) {
            LegacyStartupMode::Buffered
        } else {
            LegacyStartupMode::for_attempt(attempt_number)
        }
    }

    /// Builds the NVEncC/Rigaya command line and its stdin/stdout configuration for this executor.
    ///
    /// This constructs encoder input args, planned NVEncC options derived from the intermediate plan
    /// (resize, FRUC, HDR, profile, single-pass mode, cu sizing, GOP length, etc.), and output/mux
    /// flags based on the stage transport, returning a fully populated `NvidiaSpecializedExecutorCommand`.
    ///
    /// Errors if resolver/map/build steps fail (for example, resolving encoder input, mapping the
    /// intermediate plan, or translating output transport into muxing arguments).
    ///
    /// # Returns
    ///
    /// `NvidiaSpecializedExecutorCommand` on success.
    ///
    /// # Examples
    ///
    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
        startup_mode: LegacyStartupMode,
    ) -> io::Result<NvidiaSpecializedExecutorCommand> {
        let (input, stdin) = resolve_encoder_input(context)?;
        let planned = self.map_intermediate_plan()?;
        // Direct HLS source-pull inputs are sensitive to Rigaya's no-buffer
        // ingest mode: it keeps the stream close to the live edge, but will
        // happily discard late packets and surface that as visible timeline
        // jumps. Prefer the buffered/probed ingest profile on this path so
        // playback stays smooth instead of skipping forward under jitter.
        let low_latency_input = startup_mode.uses_low_latency_input()
            && !Self::should_prefer_buffered_source_ingest(context);
        let mut args = build_rigaya_input_args(&input, low_latency_input);
        let (output_args, stdout) = self.build_output_args(context)?;
        let force_cfr_for_staged_input =
            !matches!(context.transport.input, FrameTransport::SourcePull);
        let force_cfr_for_direct_source = Self::should_force_cfr_for_direct_source(context);
        // Derive output FPS from the source metadata and execution plan once
        // so FRUC, GOP cadence, and HLS pacing all target the same cadence.
        let output_fps = self.derive_output_fps(context, planned.enable_fruc);
        let rounded_output_fps = output_fps.round().max(1.0) as u32;
        // NVEncC's transcode-speed cap applies before FRUC generates the
        // interpolated frames. Pace FRUC jobs at half the planned output rate
        // so a 24->48 job stays near 48 fps on the wire rather than running
        // the pre-FRUC pipeline at 48 fps and overshooting downstream.
        let processing_fps_cap = if planned.enable_fruc {
            (output_fps / 2.0).round().max(1.0) as u32
        } else {
            rounded_output_fps
        };

        if self.context.denoise && matches!(&input, EncoderInput::SourceUrl { .. }) {
            append_rigaya_denoise_args(&mut args);
        }

        if matches!(&input, EncoderInput::SourceUrl { .. }) {
            if let Some((left, top, right, bottom)) =
                derive_aspect_crop(context, planned.output_resolution.as_ref())
            {
                args.push("--crop".to_string());
                args.push(format!("{},{},{},{}", left, top, right, bottom));
            }
        }

        if let Some(output_resolution) = planned.output_resolution.as_ref() {
            args.push("--output-res".to_string());
            args.push(output_resolution.as_str().to_string());
        }

        if let Some(resize_filter) = planned.resize_filter.as_deref() {
            args.push("--vpp-resize".to_string());
            args.push(resize_filter.to_string());
        }

        if planned.enable_fruc {
            args.push("--vpp-fruc".to_string());
            args.push(format!("fps={}/1", rounded_output_fps));
        }

        // Direct source-pull inputs hand container timestamps straight to
        // NVEncC because they bypass the FFmpeg normalizer. Keep CFR
        // reconciliation enabled there so minor timestamp wobble or VFR
        // irregularity does not surface as visible forward jumps. The staged
        // path also keeps CFR smoothing because its FFmpeg normalizer already
        // rebases both audio and video to a shared zero point before the
        // handoff.
        if planned.enable_fruc || force_cfr_for_staged_input || force_cfr_for_direct_source {
            args.push("--avsync".to_string());
            args.push("forcecfr".to_string());
        }

        args.extend(planned.hdr_args);

        if planned.cas_strength > 0.0 {
            args.push("--vpp-unsharp".to_string());
            args.push(format!(
                "radius=3,weight={:.2},threshold=10.0",
                planned.cas_strength
            ));
        }

        args.push("--profile".to_string());
        args.push(planned.profile.to_string());

        args.push("--multipass".to_string());
        args.push(detect_nvenc_single_pass_mode(&self.context.encoder_path).to_string());

        // Allow 64×64 CTUs for 1440p and above — the default cap of 32 is too
        // conservative at these resolutions and reduces compression efficiency.
        if planned
            .output_resolution
            .as_ref()
            .map(|r| {
                r.as_str().starts_with("2560x1440")
                    || r.as_str().starts_with("3840x2160")
                    || r.as_str().starts_with("7680x4320")
            })
            .unwrap_or(false)
        {
            args.push("--cu-max".to_string());
            args.push("64".to_string());
        }

        let gop_len = (output_fps * self.context.profile.hls_time as f64)
            .round()
            .max(1.0) as u32;

        args.extend([
            "-c".to_string(),
            "hevc".to_string(),
            "--preset".to_string(),
            self.context.profile.preset.clone(),
            "--vbr".to_string(),
            self.context.profile.bitrate.to_string(),
            "--max-bitrate".to_string(),
            self.context.profile.max_bitrate.to_string(),
            "--bframes".to_string(),
            self.context.profile.bframes.to_string(),
            "--lookahead".to_string(),
            self.context.profile.lookahead.to_string(),
            "--gop-len".to_string(),
            gop_len.to_string(),
            "--aq".to_string(),
            "--aq-temporal".to_string(),
        ]);

        if matches!(context.transport.output, FrameTransport::HlsOutput { .. }) {
            args.push("--max-procfps".to_string());
            args.push(processing_fps_cap.to_string());
        }
        args.extend(output_args);

        Ok(NvidiaSpecializedExecutorCommand {
            program: self.context.encoder_path.clone(),
            args,
            stdin,
            stdout,
        })
    }

    /// Build NVEncC output/mux arguments and determine the stdout handling based on the
    /// stage transport configured in `context`.
    ///
    /// This maps supported `FrameTransport` variants to the corresponding NVEncC
    /// muxing flags and the `StdoutMode` the executor should use:
    /// - `HlsOutput` -> HLS mux flags, `StdoutMode::Null`
    /// - `StdoutPipe` -> MPEG-TS to stdout (`-o -`), `StdoutMode::Transport(FrameTransport::StdoutPipe)`
    /// - `NamedPipe(path)` -> MPEG-TS to the named pipe path, `StdoutMode::Null`
    /// - `LocalSocket(path)` -> returns `io::ErrorKind::Unsupported`
    /// - any other transport -> returns `io::ErrorKind::InvalidInput`
    ///
    /// # Returns
    ///
    /// A tuple containing the list of NVEncC command-line arguments for muxing/output
    /// and the `StdoutMode` describing how the executor should wire its stdout.
    ///
    /// # Examples
    ///
    fn build_output_args(
        &self,
        context: &StageRuntimeContext,
    ) -> io::Result<(Vec<String>, StdoutMode)> {
        match &context.transport.output {
            FrameTransport::HlsOutput { playlist_path, .. } => Ok((
                // Compatibility path only: staged launches should hand MPEG-TS
                // off to the common FFmpeg packager instead of muxing inline.
                // Keep a short playlist window for latency, but retain recently
                // evicted segments a bit longer so slower live players such as
                // VLC can continue fetching them without stalling if they drift
                // a few seconds behind the live edge.
                vec![
                    "--audio-codec".to_string(),
                    "aac".to_string(),
                    "--format".to_string(),
                    "hls".to_string(),
                    "--strict-gop".to_string(),
                    "--mux-option".to_string(),
                    format!("hls_time:{}", self.context.profile.hls_time),
                    "--mux-option".to_string(),
                    format!("hls_list_size:{}", INLINE_HLS_LIST_SIZE),
                    "--mux-option".to_string(),
                    format!("hls_delete_threshold:{}", INLINE_HLS_DELETE_THRESHOLD),
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
                    "NVEnc executor does not yet support local-socket output at {:?}.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "NVEnc executor cannot emit {:?} as its output transport.",
                    transport
                ),
            )),
        }
    }

    fn derive_output_fps(&self, context: &StageRuntimeContext, enable_fruc: bool) -> f64 {
        let source_fps = context.source.metadata.as_ref().and_then(|m| m.source_fps);
        crate::pipeline::planned_target_frame_rate(source_fps, &context.execution_plan)
            .unwrap_or_else(|| {
                if enable_fruc {
                    UNKNOWN_FRUC_OUTPUT_FPS_FALLBACK
                } else {
                    UNKNOWN_SOURCE_FPS_FALLBACK
                }
            })
    }

    /// Converts the executor's intermediate operation plan into NVEncC-specific options used when building the encoder command.
    ///
    /// The returned `PlannedNvencOptions` encodes the chosen output resolution, any executor-side resize filter, HDR flags/profile, and whether FRUC-based interpolation should be enabled. Shader-based upscale operations (Anime4K / ArtCNN) are rejected with `io::ErrorKind::InvalidInput`.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` with kind `InvalidInput` if the intermediate plan contains a shader upscale operation that the NVIDIA specialized executor cannot render.
    ///
    /// # Examples
    ///
    fn map_intermediate_plan(&self) -> io::Result<PlannedNvencOptions> {
        debug_assert_eq!(
            self.context.intermediate_plan.executor,
            ExecutorKind::NvidiaSpecialized
        );

        let mut planned = PlannedNvencOptions::default();

        for operation in &self.context.intermediate_plan.operations {
            match operation {
                IntermediateOperation::NormalizeInput(_) => {}
                IntermediateOperation::Resize(resize) => {
                    if resize.binding.owner == IntermediateOpOwner::Executor {
                        planned.output_resolution = Some(PlannedOutputResolution::ExecutorResize(
                            resize.plan.target_resolution.clone(),
                        ));

                        if resize.plan.quality.is_some()
                            && resize.binding.accelerator.as_ref().map(|plan| plan.kind)
                                == Some(NativeAcceleratorKind::NvidiaNgxVsr)
                        {
                            if let Some(quality) = resize.plan.quality {
                                planned.resize_filter =
                                    Some(format!("algo=ngx-vsr,vsr-quality={}", quality));
                            }
                        }
                    } else {
                        // Transitional compatibility: keep the final encode size
                        // explicit even when resize is planned for shared preprocess
                        // so the staged pipeline still preserves the requested output
                        // dimensions before preprocess ownership is fully enforced.
                        planned.output_resolution =
                            Some(PlannedOutputResolution::CompatibilityFallback(
                                resize.plan.target_resolution.clone(),
                            ));
                    }
                }
                IntermediateOperation::Interpolate(interpolation) => {
                    if interpolation.binding.owner == IntermediateOpOwner::Executor
                        && interpolation
                            .binding
                            .accelerator
                            .as_ref()
                            .map(|plan| plan.kind)
                            == Some(NativeAcceleratorKind::NvidiaFruc)
                    {
                        planned.enable_fruc = true;
                    }
                }
                IntermediateOperation::HdrMetadata(metadata) => {
                    planned.apply_hdr_intent(&metadata.hdr);
                }
                IntermediateOperation::HdrTransform(transform) => {
                    planned.apply_hdr_intent(&transform.hdr);

                    if transform.binding.owner == IntermediateOpOwner::Executor
                        && transform.binding.accelerator.as_ref().map(|plan| plan.kind)
                            == Some(NativeAcceleratorKind::NvidiaTrueHdr)
                    {
                        planned.push_hdr_flag_once("--vpp-ngx-truehdr");
                    }
                }
                IntermediateOperation::Anime4k2xUpscale(op) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "NvidiaSpecializedExecutor cannot render Anime4K shader upscale \
                             for {}. Shader upscale is Universal-only and should never reach \
                             the NVIDIA specialized path.",
                            op.plan.target_resolution
                        ),
                    ));
                }
                IntermediateOperation::Artcnn2xUpscale(op) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "NvidiaSpecializedExecutor cannot render ArtCNN shader upscale \
                             for {}. Shader upscale is Universal-only and should never reach \
                             the NVIDIA specialized path.",
                            op.plan.target_resolution
                        ),
                    ));
                }
            }
        }

        planned.cas_strength = self.context.cas_strength;
        Ok(planned)
    }
}

fn derive_aspect_crop(
    context: &StageRuntimeContext,
    output_resolution: Option<&PlannedOutputResolution>,
) -> Option<(u32, u32, u32, u32)> {
    let (target_w, target_h) = parse_resolution(output_resolution?.as_str())?;
    let (source_w, source_h) = source_dimensions(context)?;

    let source_ratio = source_w as u64 * target_h as u64;
    let target_ratio = source_h as u64 * target_w as u64;

    if source_ratio == target_ratio {
        return None;
    }

    if source_ratio > target_ratio {
        let crop_w = ((source_h as u64 * target_w as u64 / target_h as u64) as u32 / 2) * 2;
        if crop_w == 0 || crop_w >= source_w {
            return None;
        }
        let total_crop = source_w - crop_w;
        let left = total_crop / 2;
        let right = total_crop - left;
        Some((left, 0, right, 0))
    } else {
        let crop_h = ((source_w as u64 * target_h as u64 / target_w as u64) as u32 / 2) * 2;
        if crop_h == 0 || crop_h >= source_h {
            return None;
        }
        let total_crop = source_h - crop_h;
        let top = total_crop / 2;
        let bottom = total_crop - top;
        Some((0, top, 0, bottom))
    }
}

fn source_dimensions(context: &StageRuntimeContext) -> Option<(u32, u32)> {
    let metadata = context.source.metadata.as_ref()?;
    match (metadata.width, metadata.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => Some((width, height)),
        _ => metadata
            .source_resolution
            .as_deref()
            .and_then(parse_resolution),
    }
}

fn parse_resolution(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

impl PlannedNvencOptions {
    /// Apply HDR output intent to the planned NVEnc options.
    ///
    /// Updates the planner to reflect HDR-related requirements from `hdr`:
    /// - sets the encoder `profile` to `"main10"` when the output bit depth is 10-bit,
    /// - ensures color/transfer/matrix/color-range NVEnc flags are present when the HDR intent requests them.
    ///
    /// # Examples
    ///
    fn apply_hdr_intent(&mut self, hdr: &HdrPlan) {
        if hdr.output_bit_depth == OutputBitDepth::Bit10 {
            self.profile = "main10";
        }

        if matches!(
            hdr.color_primaries,
            crate::graph::ColorPrimariesIntent::Bt2020
        ) {
            self.push_hdr_pair_once("--colorprim", "bt2020");
        }
        if matches!(
            hdr.transfer,
            crate::graph::TransferCharacteristicIntent::Smpte2084
        ) {
            self.push_hdr_pair_once("--transfer", "smpte2084");
        }
        if matches!(hdr.matrix, crate::graph::MatrixCoefficientsIntent::Bt2020Nc) {
            self.push_hdr_pair_once("--colormatrix", "bt2020nc");
        }
        if hdr.metadata.is_some() {
            self.push_hdr_pair_once("--colorrange", "limited");
        }
    }

    /// Appends the given HDR argument to the planned HDR argument list if it is not already present.
    ///
    /// Ensures `hdr_args` contains at most one instance of the provided flag.
    ///
    /// # Examples
    ///
    fn push_hdr_flag_once(&mut self, flag: &str) {
        if !self.hdr_args.iter().any(|arg| arg == flag) {
            self.hdr_args.push(flag.to_string());
        }
    }

    /// Append an adjacent key/value pair to `hdr_args` only if that exact pair does not already exist.
    ///
    /// This ensures the given `key` and `value` are added as consecutive entries (key then value)
    /// and avoids inserting a duplicate adjacent pair.
    ///
    /// # Examples
    ///
    fn push_hdr_pair_once(&mut self, key: &str, value: &str) {
        if !self
            .hdr_args
            .windows(2)
            .any(|window| window[0] == key && window[1] == value)
        {
            self.hdr_args.extend([key.to_string(), value.to_string()]);
        }
    }
}

impl BackendExecutor for NvidiaSpecializedExecutor {
    fn build_executor_spec(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> io::Result<ExecutorSpec> {
        let startup_mode = self.effective_startup_mode(context, attempt_number);
        let command = self.build_command(context, startup_mode)?;

        Ok(ExecutorSpec {
            kind: ExecutorKind::NvidiaSpecialized,
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
                log_label: "nvenc".to_string(),
                stderr_mode: StderrMode::Raw,
                kill_on_drop: true,
                hidden_window: true,
            },
        })
    }
}

/// Parses NVEncC `--help` output to detect which encoder features are available.
///
/// The function inspects the provided help text to determine:
/// - `has_vpp_resize`: true when vpp-resize is supported and NGX-related markers indicate resize support,
///   with behavior influenced by an explicit `ngx` feature banner when present.
/// - `has_fruc`: true when FRUC is reported enabled in feature banners or `--vpp-fruc` is present.
/// - `has_truehdr`: true when `--vpp-ngx-truehdr` is present (unless `ngx` is explicitly disabled).
/// - `has_rife`: always `false`.
///
/// # Examples
///
pub(crate) fn parse_nvenc_capabilities(help_text: &str) -> EncoderCapabilities {
    let has_resize_option = help_text.contains("--vpp-resize");
    let has_ngx_feature = help_feature_enabled(help_text, "ngx");
    let has_ngx_markers = help_text.contains("ngx-vsr")
        || help_text.contains("vsr-quality")
        || help_text.contains("--vpp-ngx-truehdr");
    let has_fruc = help_feature_enabled(help_text, "nvof fruc")
        .unwrap_or_else(|| help_text.contains("--vpp-fruc"));
    let has_vpp_resize = match has_ngx_feature {
        Some(true) => has_resize_option,
        Some(false) => false,
        None => has_resize_option && has_ngx_markers,
    };
    let has_truehdr = match has_ngx_feature {
        Some(false) => false,
        _ => help_text.contains("--vpp-ngx-truehdr"),
    };

    EncoderCapabilities {
        has_vpp_resize,
        has_fruc,
        has_truehdr,
        has_rife: false,
    }
}

/// Checks whether a named feature is reported as enabled in help text lines formatted like `"<name>: <value>"`.
///
/// The search is case-insensitive and trims surrounding whitespace. Matches only consider lines that contain a single `:` separating name and value.
///
/// # Returns
///
/// `Some(true)` if a line with the given `feature_name` is found and its value equals `"yes"`, `Some(false)` if a matching name is found with a different value, or `None` if no matching `name:` line is present.
///
/// # Examples
///
fn help_feature_enabled(help_text: &str, feature_name: &str) -> Option<bool> {
    help_text.lines().find_map(|line| {
        let lower = line.trim().to_lowercase();
        let Some((name, value)) = lower.split_once(':') else {
            return None;
        };
        (name.trim() == feature_name).then(|| value.trim() == "yes")
    })
}

/// Detects which NVEncC single-pass mode string the encoder supports by probing its `--help` output.
///
/// This spawns the encoder with `--help` and inspects stdout/stderr for known single-pass mode markers.
///
/// # Returns
///
/// `"1pass"` when the help text advertises a `1pass` single-pass mode, `"none"` otherwise (also returned on spawn failure).
///
/// # Examples
///
fn detect_nvenc_single_pass_mode(encoder_path: &Path) -> &'static str {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    let mut cmd = StdCommand::new(encoder_path);
    cmd.arg("--help");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    let output = cmd.output();

    match output {
        Ok(out) => {
            let check = |pattern: &[u8]| {
                out.stdout.windows(pattern.len()).any(|w| w == pattern)
                    || out.stderr.windows(pattern.len()).any(|w| w == pattern)
            };
            if check(b"none, 2pass-quarter, 2pass-full") {
                "none"
            } else if check(b"1pass") {
                "1pass"
            } else {
                "none"
            }
        }
        Err(_) => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_nvenc_capabilities, NvidiaSpecializedExecutor, NvidiaSpecializedExecutorContext,
    };
    use crate::graph::{
        Anime4k2xPlan, ColorPrimariesIntent, ExecutionPlan, ExecutorKind, HdrMetadataKind,
        HdrMetadataPlan, HdrPlan, HdrRequest, HdrTransformKind, HdrTransformPlan,
        IntermediateOpOwner, InterpolationDecision, InterpolationPlan, LatencyMode,
        MatrixCoefficientsIntent, OpExecutionStage, OutputBitDepth, ResizePlan,
        TransferCharacteristicIntent, VideoOp,
    };
    use crate::pipeline::{EncoderCapabilities, EncodingProfile};
    use crate::runtime::{
        BackendExecutor, FrameTransport, LegacyStartupMode, SessionOutputPaths,
        StageRuntimeContext, StdinMode, StdoutMode, TransportConfig,
    };
    use crate::source::{
        SourceClassification, SourceDescriptor, SourceKind, SourceMetadata, SourceTransport,
    };
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;

    /// Create a test StageRuntimeContext using the provided video operations and transports.
    ///
    /// The returned context uses a fixed test session id ("session-1"), an example HLS
    /// SourceDescriptor and session paths rooted at `session`. The provided `video_ops` are
    /// converted into the `execution_plan`, and the supplied `input` and `output` configure
    /// the `transport`.
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
            execution_plan: plan(video_ops),
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
                output,
            },
        }
    }

    /// Constructs an `ExecutionPlan` configured for the NVIDIA specialized executor with low-latency settings and local HLS relay enabled, using the supplied video operations.
    ///
    /// The returned plan has `executor` set to `ExecutorKind::NvidiaSpecialized`, `latency_mode` set to `LatencyMode::Low`, and `requires_local_hls_relay` set to `true`.
    ///
    /// # Examples
    ///
    fn plan(video_ops: Vec<VideoOp>) -> ExecutionPlan {
        ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops,
        }
    }

    /// Construct an `NvidiaSpecializedExecutor` using a fixed test-oriented NVEncC profile and the provided execution plan.
    ///
    /// The returned executor is initialized with a small default encoding profile, a placeholder encoder path (`missing-nvencc.exe`),
    /// default encoder capabilities that enable NGX/FRUC/TrueHDR, and an intermediate plan derived from `plan`.
    ///
    /// # Examples
    ///
    fn executor_for_plan(plan: &ExecutionPlan) -> NvidiaSpecializedExecutor {
        NvidiaSpecializedExecutor::new(NvidiaSpecializedExecutorContext {
            profile: EncodingProfile {
                bitrate: 50_000,
                max_bitrate: 75_000,
                preset: "p4".to_string(),
                lookahead: 8,
                bframes: 3,
                hls_time: 1,
            },
            encoder_path: PathBuf::from("missing-nvencc.exe"),
            capabilities: EncoderCapabilities {
                has_vpp_resize: true,
                has_fruc: true,
                has_truehdr: true,
                has_rife: false,
            },
            intermediate_plan: plan.to_intermediate_plan(),
            denoise: false,
            cas_strength: 0.0,
        })
    }

    fn executor_for_plan_with_denoise(
        plan: &ExecutionPlan,
        denoise: bool,
    ) -> NvidiaSpecializedExecutor {
        NvidiaSpecializedExecutor::new(NvidiaSpecializedExecutorContext {
            profile: EncodingProfile {
                bitrate: 50_000,
                max_bitrate: 75_000,
                preset: "p4".to_string(),
                lookahead: 8,
                bframes: 3,
                hls_time: 1,
            },
            encoder_path: PathBuf::from("missing-nvencc.exe"),
            capabilities: EncoderCapabilities {
                has_vpp_resize: true,
                has_fruc: true,
                has_truehdr: true,
                has_rife: false,
            },
            intermediate_plan: plan.to_intermediate_plan(),
            denoise,
            cas_strength: 0.0,
        })
    }

    /// Constructs an `NvidiaSpecializedExecutor` preconfigured with a sensible default
    /// encoding profile and the supplied intermediate execution plan.
    ///
    /// The returned executor uses a default NVEncC encoding profile (50_000 bitrate,
    /// 75_000 max bitrate, preset "p4", lookahead 8, 3 B-frames, 1s HLS segment)
    /// and a placeholder encoder path; `intermediate_plan` is embedded into the
    /// executor context and used when building NVEncC command flags.
    ///
    /// # Examples
    ///
    fn executor_with_intermediate_plan(
        intermediate_plan: crate::graph::IntermediateExecutionPlan,
    ) -> NvidiaSpecializedExecutor {
        NvidiaSpecializedExecutor::new(NvidiaSpecializedExecutorContext {
            profile: EncodingProfile {
                bitrate: 50_000,
                max_bitrate: 75_000,
                preset: "p4".to_string(),
                lookahead: 8,
                bframes: 3,
                hls_time: 1,
            },
            encoder_path: PathBuf::from("missing-nvencc.exe"),
            capabilities: EncoderCapabilities {
                has_vpp_resize: true,
                has_fruc: true,
                has_truehdr: true,
                has_rife: false,
            },
            intermediate_plan,
            denoise: false,
            cas_strength: 0.0,
        })
    }

    /// Constructs an `NvidiaSpecializedExecutor` and its corresponding `StageRuntimeContext`
    /// from a list of planned `VideoOp`s and input/output `FrameTransport`s.
    ///
    /// The returned executor is created for the execution plan embedded in the returned
    /// `StageRuntimeContext`.
    ///
    /// # Examples
    ///
    fn executor_context(
        video_ops: Vec<VideoOp>,
        input: FrameTransport,
        output: FrameTransport,
    ) -> (NvidiaSpecializedExecutor, StageRuntimeContext) {
        let context = context(video_ops, input, output);
        let executor = executor_for_plan(&context.execution_plan);
        (executor, context)
    }

    /// Create an `NvidiaSpecializedExecutor` and a matching `StageRuntimeContext` configured

    /// with the provided intermediate execution plan, input/output transports, and video ops.

    ///

    /// The returned executor is constructed from `intermediate_plan`; the returned runtime

    /// context is initialized from `video_ops`, `input`, and `output`.

    ///

    /// # Examples

    ///

    fn executor_context_with_intermediate_plan(
        video_ops: Vec<VideoOp>,
        input: FrameTransport,
        output: FrameTransport,
        intermediate_plan: crate::graph::IntermediateExecutionPlan,
    ) -> (NvidiaSpecializedExecutor, StageRuntimeContext) {
        let context = context(video_ops, input, output);
        let executor = executor_with_intermediate_plan(intermediate_plan);
        (executor, context)
    }

    /// Checks whether a slice of strings contains the given sequence of string slices contiguously.
    ///
    /// # Returns
    ///
    /// `true` if `expected` appears as a contiguous subsequence within `args`, `false` otherwise.
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
    fn parses_nvenc_capabilities_from_feature_banner() {
        let help = r#"
NVEnc (x64) 9.14
 others
  ngx        : yes
  nvof fruc  : yes
   --vpp-resize <string>
   --vpp-fruc [<param1>=<value>]
   --vpp-ngx-truehdr
"#;

        let caps = parse_nvenc_capabilities(help);

        assert!(caps.has_vpp_resize);
        assert!(caps.has_fruc);
        assert!(caps.has_truehdr);
    }

    #[test]
    fn parse_nvenc_capabilities_does_not_assume_ngx_resize_from_generic_flag_alone() {
        let help = r#"
NVEnc (x64) 9.14
   --vpp-resize <string>
"#;

        let caps = parse_nvenc_capabilities(help);

        assert!(!caps.has_vpp_resize);
        assert!(!caps.has_truehdr);
    }

    #[test]
    fn build_command_maps_executor_video_ops_to_nvenc_flags() {
        let (executor, context) = executor_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(3),
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
                    metadata: Some(HdrMetadataPlan {
                        kind: HdrMetadataKind::Hdr10Static,
                        stage: OpExecutionStage::Executor,
                    }),
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::NvidiaTrueHdrTonemapToHdr10,
                        stage: OpExecutionStage::Executor,
                    }),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        let command = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert_eq!(
            command.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(command.stdout, StdoutMode::Null);
        assert!(has_subsequence(&command.args, &["--avsw", "-i", "-"]));
        assert!(has_subsequence(
            &command.args,
            &["--output-res", "3840x2160"]
        ));
        assert!(has_subsequence(
            &command.args,
            &["--vpp-resize", "algo=ngx-vsr,vsr-quality=3"]
        ));
        assert!(has_subsequence(&command.args, &["--vpp-fruc", "fps=60/1"]));
        assert!(has_subsequence(&command.args, &["--avsync", "forcecfr"]));
        assert!(has_subsequence(&command.args, &["--vpp-ngx-truehdr"]));
        assert!(has_subsequence(&command.args, &["--profile", "main10"]));
        assert!(has_subsequence(&command.args, &["--lookahead", "8"]));
        assert!(has_subsequence(&command.args, &["--aq", "--aq-temporal"]));
        assert!(has_subsequence(&command.args, &["--format", "hls"]));
        assert!(command.args.iter().any(|arg| arg == "--strict-gop"));
        assert!(has_subsequence(&command.args, &["--gop-len", "60"]));
        assert!(has_subsequence(&command.args, &["--max-procfps", "30"]));
        assert!(has_subsequence(
            &command.args,
            &[
                "--mux-option",
                &format!("hls_list_size:{}", super::INLINE_HLS_LIST_SIZE)
            ]
        ));
        assert!(has_subsequence(
            &command.args,
            &[
                "--mux-option",
                &format!(
                    "hls_delete_threshold:{}",
                    super::INLINE_HLS_DELETE_THRESHOLD
                )
            ]
        ));
        assert!(has_subsequence(
            &command.args,
            &["--mux-option", "hls_flags:delete_segments"]
        ));
    }

    #[test]
    fn preprocess_resize_falls_back_to_output_res_without_ngx_resize_filter() {
        let (executor, context) = executor_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "2560x1440".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Preprocess,
                }),
            ],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        let command = executor
            .build_command(&context, LegacyStartupMode::Buffered)
            .unwrap();

        assert_eq!(command.stdin, StdinMode::Null);
        assert!(has_subsequence(
            &command.args,
            &[
                "-i",
                "http://127.0.0.1:14002/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8"
            ]
        ));
        assert!(has_subsequence(
            &command.args,
            &["--output-res", "2560x1440"]
        ));
        assert!(!command.args.iter().any(|arg| arg.contains("ngx-vsr")));
        assert!(has_subsequence(&command.args, &["--multipass", "none"]));
        assert!(has_subsequence(&command.args, &["--avsync", "forcecfr"]));
    }

    #[test]
    fn build_command_switches_to_mpegts_pipe_output_for_common_packager() {
        let (executor, context) = executor_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let command = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert_eq!(
            command.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(
            command.stdout,
            StdoutMode::Transport(FrameTransport::StdoutPipe)
        );
        assert!(has_subsequence(&command.args, &["--format", "mpegts"]));
        assert!(has_subsequence(&command.args, &["-o", "-"]));
        assert!(has_subsequence(&command.args, &["--avsw", "-i", "-"]));
        assert!(!command.args.iter().any(|arg| arg == "hls"));
        assert!(!command.args.iter().any(|arg| arg == "--strict-gop"));
    }

    #[test]
    fn build_command_prefers_buffered_ingest_for_direct_hls_source_pull() {
        let (executor, context) = executor_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        let command = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(
            !command
                .args
                .windows(2)
                .any(|window| window == ["--input-option", "fflags:nobuffer"]),
            "direct HLS source-pull should avoid nobuffer ingest: {:?}",
            command.args
        );
        assert!(has_subsequence(&command.args, &["--input-analyze", "4"]));
        assert!(has_subsequence(
            &command.args,
            &["--input-probesize", "1000000"]
        ));
        assert!(has_subsequence(&command.args, &["--avsync", "forcecfr"]));
        assert!(has_subsequence(&command.args, &["--gop-len", "30"]));
        assert!(has_subsequence(&command.args, &["--max-procfps", "30"]));
        assert!(command.args.iter().any(|arg| arg == "--strict-gop"));

        let spec = executor.build_executor_spec(&context, 1).unwrap();
        assert_eq!(spec.startup_label.as_deref(), Some("buffered"));
    }

    #[test]
    fn build_command_forces_cfr_for_direct_non_hls_source_pull() {
        let (executor, mut context) = executor_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        context.source.classification.kind = SourceKind::Other;
        context.source.original_url = "https://example.com/video.mp4".to_string();
        context.source.runtime_url = "https://example.com/video.mp4".to_string();

        let command = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(has_subsequence(&command.args, &["--avsync", "forcecfr"]));
    }

    #[test]
    fn build_command_limits_hls_transcode_speed_to_known_output_fps() {
        let (executor, mut context) = executor_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        context.source.metadata = Some(SourceMetadata {
            source_fps: Some(24.0),
            ..SourceMetadata::default()
        });

        let command = executor
            .build_command(&context, LegacyStartupMode::Buffered)
            .unwrap();

        assert!(has_subsequence(&command.args, &["--gop-len", "24"]));
        assert!(has_subsequence(&command.args, &["--max-procfps", "24"]));
    }

    #[test]
    fn build_command_limits_fruc_hls_transcode_speed_to_double_source_fps() {
        let (executor, mut context) = executor_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 48,
                    decision: InterpolationDecision::native_backend(OpExecutionStage::Executor),
                }),
            ],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        context.source.metadata = Some(SourceMetadata {
            source_fps: Some(24.0),
            ..SourceMetadata::default()
        });

        let command = executor
            .build_command(&context, LegacyStartupMode::Buffered)
            .unwrap();

        assert!(has_subsequence(&command.args, &["--vpp-fruc", "fps=48/1"]));
        assert!(has_subsequence(&command.args, &["--gop-len", "48"]));
        assert!(has_subsequence(&command.args, &["--max-procfps", "24"]));
    }

    #[test]
    fn build_command_center_crops_ultrawide_source_before_direct_resize() {
        let mut context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "1920x1080".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        context.source.metadata = Some(SourceMetadata {
            width: Some(2560),
            height: Some(1080),
            source_resolution: Some("2560x1080".to_string()),
            source_fps: Some(60.0),
            ..SourceMetadata::default()
        });

        let executor = executor_for_plan(&context.execution_plan);
        let command = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(has_subsequence(&command.args, &["--crop", "320,0,320,0"]));
        assert!(has_subsequence(
            &command.args,
            &["--output-res", "1920x1080"]
        ));
    }

    #[test]
    fn build_command_applies_gpu_denoise_only_on_source_pull_inputs() {
        let direct_context = context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        let direct_executor = executor_for_plan_with_denoise(&direct_context.execution_plan, true);
        let direct_command = direct_executor
            .build_command(&direct_context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(has_subsequence(
            &direct_command.args,
            &[
                "--vpp-convolution3d",
                "ythresh=4,cthresh=3,t_ythresh=6,t_cthresh=4.5",
            ]
        ));

        let staged_context = context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let staged_executor = executor_for_plan_with_denoise(&staged_context.execution_plan, true);
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
    fn build_executor_spec_keeps_nvenc_stage_identity() {
        let (executor, context) = executor_context(
            vec![VideoOp::NormalizeInput],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let spec = executor.build_executor_spec(&context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::NvidiaSpecialized);
        assert_eq!(
            spec.process.stage,
            crate::runtime::PipelineStageId::Executor
        );
        assert_eq!(
            spec.process.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(
            spec.process.stdout,
            StdoutMode::Transport(FrameTransport::StdoutPipe)
        );
    }

    #[test]
    fn build_command_uses_intermediate_bindings_as_the_nvenc_render_boundary() {
        let context = context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(3),
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
                    metadata: Some(HdrMetadataPlan {
                        kind: HdrMetadataKind::Hdr10Static,
                        stage: OpExecutionStage::Executor,
                    }),
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::NvidiaTrueHdrTonemapToHdr10,
                        stage: OpExecutionStage::Executor,
                    }),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        let mut intermediate_plan = context.execution_plan.to_intermediate_plan();

        for operation in &mut intermediate_plan.operations {
            match operation {
                crate::graph::IntermediateOperation::Resize(resize) => {
                    resize.binding.owner = IntermediateOpOwner::SharedPreprocess;
                    resize.binding.accelerator = None;
                }
                crate::graph::IntermediateOperation::Interpolate(interpolation) => {
                    interpolation.binding.owner = IntermediateOpOwner::Deferred;
                    interpolation.binding.accelerator = None;
                }
                crate::graph::IntermediateOperation::HdrTransform(transform) => {
                    transform.binding.owner = IntermediateOpOwner::SharedPreprocess;
                    transform.binding.accelerator = None;
                }
                _ => {}
            }
        }

        let (executor, context) = executor_context_with_intermediate_plan(
            context.execution_plan.video_ops.clone(),
            context.transport.input.clone(),
            context.transport.output.clone(),
            intermediate_plan,
        );
        let command = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .unwrap();

        assert!(has_subsequence(
            &command.args,
            &["--output-res", "3840x2160"]
        ));
        assert!(!command.args.iter().any(|arg| arg.contains("ngx-vsr")));
        assert!(!command.args.iter().any(|arg| arg == "--vpp-fruc"));
        assert!(has_subsequence(&command.args, &["--avsync", "forcecfr"]));
        assert!(!command.args.iter().any(|arg| arg == "--vpp-ngx-truehdr"));
        assert!(has_subsequence(&command.args, &["--profile", "main10"]));
    }

    #[test]
    fn build_command_rejects_shader_upscale_ops_in_nvidia_specialized_path() {
        let (executor, context) = executor_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Anime4k2xUpscale(Anime4k2xPlan {
                    target_resolution: "3840x2160".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );

        let error = executor
            .build_command(&context, LegacyStartupMode::LowLatency)
            .expect_err("shader ops must not be silently dropped by the NVIDIA executor");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let message = error.to_string();
        assert!(message.contains("Anime4K"));
        assert!(message.contains("Universal-only"));
        assert!(message.contains("NVIDIA specialized path"));
    }
}
