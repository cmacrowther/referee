use std::io;
use std::path::{Path, PathBuf};

use crate::graph::{
    parse_resolution, ExecutorKind, HdrPlan, IntermediateExecutionPlan, IntermediateOpOwner,
    IntermediateOperation, NativeAcceleratorKind,
};
use crate::pipeline::{EncoderCapabilities, EncodingProfile};
use crate::runtime::{
    BackendExecutor, ExecutorSpec, PipelineStageId, ProcessSpec, StageRuntimeContext, StartupMode,
};

use super::{
    append_rigaya_denoise_args, build_rigaya_input_args, build_rigaya_output_args,
    push_rigaya_hdr_color_args, resolve_encoder_input, rigaya_aspect_crop_args,
    rigaya_prefers_buffered_ingest, rigaya_processing_fps_cap, rigaya_startup_mode,
    segment_aligned_gop, RigayaInlineHls, UNKNOWN_SOURCE_FPS_FALLBACK,
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
    pub intermediate_plan: IntermediateExecutionPlan,
    /// Apply GPU-side denoising equivalent to FFmpeg `hqdn3d` when this
    /// executor reads from the source directly rather than a staged normalizer.
    pub denoise: bool,
    /// CAS sharpening strength applied via `--vpp-unsharp`. Range [0.0, 1.0]; 0.0 disables.
    pub cas_strength: f32,
}

pub struct NvidiaSpecializedExecutor {
    context: NvidiaSpecializedExecutorContext,
}

/// NVIDIA frame generation multiplies the frame rate by this whole number.
const FRAMEGEN_MULTIPLIER: u32 = 2;
/// NVIDIA frame generation quality. `high` costs about 1.5x `medium`: it runs
/// in real time below 4K on RTX 40-class GPUs but leaves little headroom at 4K.
fn framegen_mode(output_height: Option<u32>) -> &'static str {
    match output_height {
        Some(height) if height < 2160 => "high",
        _ => "medium",
    }
}

/// Inline HLS keeps a short playlist window for latency but retains recently
/// evicted segments, so slower live players such as VLC can still fetch them
/// if they drift a few seconds behind the live edge.
fn inline_hls(hls_time: u32) -> RigayaInlineHls {
    RigayaInlineHls {
        hls_time,
        strict_gop: true,
        retain_evicted_segments: true,
    }
}

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
    enable_framegen: bool,
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
            enable_framegen: false,
            profile: "main",
            cas_strength: 0.0,
        }
    }
}

impl NvidiaSpecializedExecutor {
    pub fn new(context: NvidiaSpecializedExecutorContext) -> Self {
        Self { context }
    }

    /// Builds the NVEncC process for this stage: source ingest, the NVEncC
    /// options derived from the intermediate plan (resize, FRUC, HDR, profile,
    /// GOP, pacing), and output muxing for the stage transport.
    ///
    /// # Errors
    ///
    /// Fails when the plan contains an operation NVEncC cannot render, or the
    /// stage transport is not supported.
    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
        startup_mode: StartupMode,
    ) -> io::Result<ProcessSpec> {
        let input = resolve_encoder_input(context)?;
        let planned = self.map_intermediate_plan()?;
        let low_latency_input =
            startup_mode.uses_low_latency_input() && !rigaya_prefers_buffered_ingest(context);
        let mut args = build_rigaya_input_args(&input, low_latency_input);
        let output_args = build_rigaya_output_args(
            &context.transport.output,
            inline_hls(self.context.profile.hls_time),
        )?;
        // Derive output FPS from the execution plan once so FRUC, GOP cadence,
        // and HLS pacing all target the same cadence. With FRUC this is the
        // interpolation target, which is known even when the source rate is not.
        let output_fps = context
            .planned_output_fps()
            .unwrap_or(UNKNOWN_SOURCE_FPS_FALLBACK);
        if input.is_source_url() {
            if self.context.denoise {
                append_rigaya_denoise_args(&mut args);
            }
            args.extend(rigaya_aspect_crop_args(
                context,
                planned
                    .output_resolution
                    .as_ref()
                    .map(PlannedOutputResolution::as_str),
            ));
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
            args.push(format!("fps={}/1", output_fps.round().max(1.0) as u32));
        }

        if planned.enable_framegen {
            let output_height = planned
                .output_resolution
                .as_ref()
                .map(|resolution| resolution.as_str().to_string())
                .or_else(|| {
                    context
                        .source
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.source_resolution.clone())
                })
                .and_then(|resolution| parse_resolution(&resolution))
                .map(|(_, height)| height);
            args.push("--vpp-nvvfx-framegen".to_string());
            args.push(format!(
                "multiplier={},mode={}",
                FRAMEGEN_MULTIPLIER,
                framegen_mode(output_height)
            ));
        }

        // Always reconcile to CFR. Direct source pulls hand container
        // timestamps straight to NVEncC, so minor wobble or VFR irregularity
        // would otherwise surface as visible forward jumps; staged input has
        // already been rebased by the FFmpeg normalizer; and FRUC needs a
        // constant input cadence.
        args.push("--avsync".to_string());
        args.push("forcecfr".to_string());

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
            .and_then(|resolution| parse_resolution(resolution.as_str()))
            .is_some_and(|(_, height)| height >= 1440)
        {
            args.push("--cu-max".to_string());
            args.push("64".to_string());
        }

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
            segment_aligned_gop(output_fps, self.context.profile.hls_time).to_string(),
            "--aq".to_string(),
            "--aq-temporal".to_string(),
        ]);

        // Paces input frames (24 fps for a 24->60 FRUC job), see
        // `rigaya_processing_fps_cap`.
        if let Some(processing_fps_cap) = rigaya_processing_fps_cap(context) {
            args.push("--max-procfps".to_string());
            args.push(processing_fps_cap.to_string());
        }
        args.extend(output_args);

        Ok(ProcessSpec::new(
            PipelineStageId::Executor,
            &self.context.encoder_path,
            args,
            context.transport.clone(),
            "nvenc",
        ))
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
                    if interpolation.binding.owner == IntermediateOpOwner::Executor {
                        match interpolation
                            .binding
                            .accelerator
                            .as_ref()
                            .map(|plan| plan.kind)
                        {
                            Some(NativeAcceleratorKind::NvidiaFruc) => planned.enable_fruc = true,
                            Some(NativeAcceleratorKind::NvidiaFramegen) => {
                                planned.enable_framegen = true
                            }
                            _ => {}
                        }
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
        if push_rigaya_hdr_color_args(&mut self.hdr_args, hdr) {
            self.profile = "main10";
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
}

impl BackendExecutor for NvidiaSpecializedExecutor {
    fn build_executor_spec(
        &self,
        context: &StageRuntimeContext,
        attempt_number: u32,
    ) -> io::Result<ExecutorSpec> {
        let startup_mode = rigaya_startup_mode(context, attempt_number);
        Ok(ExecutorSpec {
            process: self.build_command(context, startup_mode)?,
            startup_mode,
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
    let has_native_double = help_feature_enabled(help_text, "nvvfx") != Some(false)
        && help_text.contains("--vpp-nvvfx-framegen");

    EncoderCapabilities {
        has_vpp_resize,
        has_fruc,
        has_truehdr,
        has_rife: false,
        has_native_double,
        has_hdr_tonemap: false,
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
/// This reads the encoder's `--help` output (shared with capability detection through the probe
/// cache, so it is spawned at most once per binary) and inspects stdout/stderr for known
/// single-pass mode markers.
///
/// # Returns
///
/// `"1pass"` when the help text advertises a `1pass` single-pass mode, `"none"` otherwise (also returned on spawn failure).
///
/// # Examples
///
fn detect_nvenc_single_pass_mode(encoder_path: &Path) -> &'static str {
    let Ok(out) = crate::process::cached_probe_output(encoder_path, &["--help"]) else {
        return "none";
    };
    let check = |pattern: &[u8]| {
        out.stdout.windows(pattern.len()).any(|w| w == pattern)
            || out.stderr.windows(pattern.len()).any(|w| w == pattern)
    };
    if check(b"none, 2pass-quarter, 2pass-full") || !check(b"1pass") {
        "none"
    } else {
        "1pass"
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
        MatrixCoefficientsIntent, NativeInterpolationEngine, OpExecutionStage, OutputBitDepth,
        ResizePlan, TransferCharacteristicIntent, VideoOp,
    };
    use crate::pipeline::EncodingProfile;
    use crate::runtime::{
        BackendExecutor, FrameTransport, SessionOutputPaths, StageRuntimeContext, StartupMode,
        TransportConfig,
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
    fn parse_nvenc_capabilities_reads_nvidia_framegen_from_banner_and_flag() {
        let help = |nvvfx: &str| {
            format!(
                "NVEncC (x64) 9.38\n others\n  nvvfx      : {nvvfx}\n   --vpp-nvvfx-framegen [<param1>=<value>]\n"
            )
        };

        assert!(parse_nvenc_capabilities(&help("yes")).has_native_double);
        assert!(!parse_nvenc_capabilities(&help("no")).has_native_double);
        assert!(!parse_nvenc_capabilities("  nvvfx      : yes\n").has_native_double);
    }

    #[test]
    fn build_command_renders_nvidia_framegen_doubling() {
        for (output_resolution, mode) in [("1920x1080", "high"), ("3840x2160", "medium")] {
            let (executor, mut context) = executor_context(
                vec![
                    VideoOp::NormalizeInput,
                    VideoOp::Resize(ResizePlan {
                        target_resolution: output_resolution.to_string(),
                        quality: Some(2),
                        stage: OpExecutionStage::Executor,
                    }),
                    VideoOp::Interpolate(InterpolationPlan {
                        target_fps: 48,
                        decision: InterpolationDecision::native_engine(
                            OpExecutionStage::Executor,
                            NativeInterpolationEngine::NvidiaFramegen,
                        ),
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
                source_resolution: Some("1280x720".to_string()),
                ..SourceMetadata::default()
            });

            let command = executor
                .build_command(&context, StartupMode::Buffered)
                .unwrap();

            let framegen = format!("multiplier=2,mode={mode}");
            assert!(has_subsequence(
                &command.args,
                &["--vpp-nvvfx-framegen", framegen.as_str()]
            ));
            assert!(!command.args.iter().any(|arg| arg == "--vpp-fruc"));
            assert!(has_subsequence(&command.args, &["--gop-len", "48"]));
            assert!(has_subsequence(&command.args, &["--max-procfps", "26"]));
        }
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
            .build_command(&context, StartupMode::LowLatency)
            .unwrap();

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
        // No source metadata, so no pacing cap (see
        // build_command_skips_transcode_speed_cap_when_source_fps_is_unknown).
        assert!(!command.args.iter().any(|arg| arg == "--max-procfps"));
        assert!(has_subsequence(
            &command.args,
            &["--mux-option", "hls_list_size:8"]
        ));
        assert!(has_subsequence(
            &command.args,
            &["--mux-option", "hls_delete_threshold:8"]
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
            .build_command(&context, StartupMode::Buffered)
            .unwrap();

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
            .build_command(&context, StartupMode::LowLatency)
            .unwrap();

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
            .build_command(&context, StartupMode::LowLatency)
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
        assert!(command.args.iter().any(|arg| arg == "--strict-gop"));

        let spec = executor.build_executor_spec(&context, 1).unwrap();
        assert_eq!(spec.startup_mode, StartupMode::Buffered);
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
            .build_command(&context, StartupMode::LowLatency)
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
            .build_command(&context, StartupMode::Buffered)
            .unwrap();

        assert!(has_subsequence(&command.args, &["--gop-len", "24"]));
        // ceil(24 * 1.05): enough headroom to recover from stalls.
        assert!(has_subsequence(&command.args, &["--max-procfps", "26"]));
    }

    #[test]
    fn build_command_skips_transcode_speed_cap_when_source_fps_is_unknown() {
        let (executor, context) = executor_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::native_backend(OpExecutionStage::Executor),
                }),
            ],
            FrameTransport::SourcePull,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        assert!(context.source.metadata.is_none());

        let command = executor
            .build_command(&context, StartupMode::Buffered)
            .unwrap();

        // The fallback cadence still drives FRUC and GOP length, but it must
        // not pace the encode: a 60 fps source capped at 30 runs at half speed.
        assert!(has_subsequence(&command.args, &["--vpp-fruc", "fps=60/1"]));
        assert!(has_subsequence(&command.args, &["--gop-len", "60"]));
        assert!(
            !command.args.iter().any(|arg| arg == "--max-procfps"),
            "unknown source fps must not cap transcode speed: {:?}",
            command.args
        );
    }

    #[test]
    fn build_command_paces_fruc_hls_transcode_at_source_fps_for_any_target() {
        for (target_fps, gop) in [(30, "30"), (60, "60"), (144, "144")] {
            let (executor, mut context) = executor_context(
                vec![
                    VideoOp::NormalizeInput,
                    VideoOp::Interpolate(InterpolationPlan {
                        target_fps,
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
                .build_command(&context, StartupMode::Buffered)
                .unwrap();

            let fruc = format!("fps={target_fps}/1");
            assert!(has_subsequence(
                &command.args,
                &["--vpp-fruc", fruc.as_str()]
            ));
            assert!(has_subsequence(&command.args, &["--gop-len", gop]));
            // ceil(24 * 1.05): the cap counts input frames, before FRUC.
            assert!(has_subsequence(&command.args, &["--max-procfps", "26"]));
        }
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
            .build_command(&context, StartupMode::LowLatency)
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
            .build_command(&direct_context, StartupMode::LowLatency)
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
            .build_command(&staged_context, StartupMode::LowLatency)
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

        assert_eq!(
            spec.process.stage,
            crate::runtime::PipelineStageId::Executor
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
            .build_command(&context, StartupMode::LowLatency)
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
            .build_command(&context, StartupMode::LowLatency)
            .expect_err("shader ops must not be silently dropped by the NVIDIA executor");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let message = error.to_string();
        assert!(message.contains("Anime4K"));
        assert!(message.contains("Universal-only"));
        assert!(message.contains("NVIDIA specialized path"));
    }
}
