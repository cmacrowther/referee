//! VCEEncC executor: the AMD counterpart of `NvidiaSpecializedExecutor`.
//!
//! Renders the executor-owned operations of an `AmdSpecialized` plan as
//! VCEEncC options: AMF FSR resize, AMF FRC (AMD Fluid Motion) frame doubling,
//! libplacebo SDR-to-HDR10 with HDR10 metadata, plus the denoise, sharpening,
//! aspect crop, CFR, and live pacing the NVIDIA path uses. When VCEEncC fails
//! to start, the pipeline retries the session on the Universal FFmpeg executor
//! (`SessionPipeline::switch_to_universal_fallback`).

use std::io;
use std::path::{Path, PathBuf};

use crate::graph::{
    ExecutorKind, IntermediateExecutionPlan, IntermediateOpOwner, IntermediateOperation,
    NativeAcceleratorKind,
};
use crate::pipeline::EncodingProfile;
use crate::runtime::{
    BackendExecutor, ExecutorSpec, PipelineStageId, ProcessSpec, StageRuntimeContext, StartupMode,
};

use super::{
    append_rigaya_denoise_args, build_rigaya_input_args, build_rigaya_output_args,
    push_rigaya_hdr_color_args, resolve_encoder_input, rigaya_aspect_crop_args,
    rigaya_prefers_buffered_ingest, rigaya_processing_fps_cap, rigaya_startup_mode,
    segment_aligned_gop, RigayaInlineHls, UNKNOWN_SOURCE_FPS_FALLBACK,
};

/// AMF FRC settings: VCEEncC's defaults, spelled out. No measurements yet show
/// where the costlier `super` profile stays real time.
const FRC_ARGS: &str = "profile=high,search=native";
/// SDR to HDR10 through libplacebo. `spline` is one of the tone curves with an
/// inverse, so highlights expand toward the 1000-nit target; `bt2390` has no
/// inverse and would leave SDR levels unchanged inside the PQ container.
const HDR_TONEMAP_ARGS: &str =
    "src_csp=sdr,dst_csp=hdr10,tonemapping_function=spline,inverse_tone_mapping=true,dst_max=1000";
/// HDR10 static metadata matching the 1000-nit target: MaxCLL,MaxFALL and a
/// BT.2020 mastering display from 0.005 to 1000 nits.
const HDR10_MAX_CLL: &str = "1000,400";
const HDR10_MASTER_DISPLAY: &str =
    "G(8500,39850)B(6550,2300)R(35400,14600)WP(15635,16450)L(10000000,50)";

/// Whether this VCEEncC offers AMF FSR 1.1 (`amf_fsr_11`) for `--vpp-resize`.
/// Reads the cached `--help` probe.
pub fn detect_vceenc_amf_fsr11(encoder_path: &Path) -> bool {
    crate::process::cached_probe_output(encoder_path, &["--help"])
        .map(|output| {
            String::from_utf8_lossy(&output.stdout).contains("amf_fsr_11")
                || String::from_utf8_lossy(&output.stderr).contains("amf_fsr_11")
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct AmdExecutorContext {
    pub profile: EncodingProfile,
    pub encoder_path: PathBuf,
    /// Log label (`"vceenc"`).
    pub encoder_backend: String,
    /// An `AmdSpecialized` plan.
    pub intermediate_plan: IntermediateExecutionPlan,
    /// VCEEncC offers AMF FSR 1.1 (see [`detect_vceenc_amf_fsr11`]).
    pub amf_fsr11: bool,
    /// Apply GPU-side denoising equivalent to FFmpeg `hqdn3d` when this
    /// executor reads from the source directly rather than a staged normalizer.
    pub denoise: bool,
    /// CAS sharpening strength applied via `--vpp-unsharp`. Range [0.0, 1.0];
    /// 0.0 disables.
    pub cas_strength: f32,
}

pub struct AmdExecutor {
    context: AmdExecutorContext,
}

/// AMF HQ scaler settings for one upscale.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AmfResize {
    algo: &'static str,
    /// `--vpp-scaler-sharpness` (RCAS strength, 0.0–2.0).
    sharpness: f32,
}

impl AmfResize {
    /// Upscale quality 1 (fastest) to 4 (best), the AMD analogue of NGX VSR's
    /// quality levels: FSR 1.1 where VCEEncC offers it with RCAS sharpening
    /// rising by quality; quality 1 keeps FSR 1.0.
    fn for_quality(quality: Option<u8>, fsr11: bool) -> Self {
        let best = if fsr11 { "amf_fsr_11" } else { "amf_fsr" };
        let (algo, sharpness) = match quality.unwrap_or(2) {
            1 => ("amf_fsr", 0.5),
            2 => (best, 0.5),
            3 => (best, 0.75),
            _ => (best, 1.0),
        };
        Self { algo, sharpness }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
struct PlannedVceEncOptions {
    /// The final encode size; set for any resize, executor-owned or not, so
    /// the output dimensions stay explicit.
    output_resolution: Option<String>,
    resize: Option<AmfResize>,
    enable_frc: bool,
    hdr_tonemap: bool,
    hdr10_metadata: bool,
    hdr_args: Vec<String>,
    ten_bit: bool,
}

/// Inline HLS keeps a short playlist window but retains recently evicted
/// segments for slower players, as on NVIDIA. VCEEncC has no `--strict-gop`.
fn inline_hls(hls_time: u32) -> RigayaInlineHls {
    RigayaInlineHls {
        hls_time,
        strict_gop: false,
        retain_evicted_segments: true,
    }
}

/// Map NVEncC-style preset identifiers (`"p1"`..`"p7"`) to VCEEncC presets.
fn translate_vceenc_preset(preset: &str) -> &'static str {
    match preset {
        "p1" | "p2" => "fast",
        "p6" | "p7" => "slow",
        _ => "balanced",
    }
}

impl AmdExecutor {
    pub fn new(context: AmdExecutorContext) -> Self {
        Self { context }
    }

    /// Builds the VCEEncC process for this stage: source ingest, the options
    /// derived from the intermediate plan (resize, FRC, HDR, profile, GOP,
    /// pacing), and output muxing for the stage transport.
    ///
    /// # Errors
    ///
    /// Fails when the plan contains an operation VCEEncC cannot render, or the
    /// stage transport is not supported.
    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
        startup_mode: StartupMode,
    ) -> io::Result<ProcessSpec> {
        let profile = &self.context.profile;
        let input = resolve_encoder_input(context)?;
        let planned = self.map_intermediate_plan()?;
        let low_latency_input =
            startup_mode.uses_low_latency_input() && !rigaya_prefers_buffered_ingest(context);
        let mut args = build_rigaya_input_args(&input, low_latency_input);
        let output_args =
            build_rigaya_output_args(&context.transport.output, inline_hls(profile.hls_time))?;
        // With FRC this is double the source rate.
        let output_fps = context
            .planned_output_fps()
            .unwrap_or(UNKNOWN_SOURCE_FPS_FALLBACK);

        if input.is_source_url() {
            if self.context.denoise {
                append_rigaya_denoise_args(&mut args);
            }
            args.extend(rigaya_aspect_crop_args(
                context,
                planned.output_resolution.as_deref(),
            ));
        }

        // VCEEncC applies its filters in a fixed order (tone mapping, denoise,
        // resize, sharpening, FRC last at the output size), whatever the
        // argument order.
        if planned.hdr_tonemap {
            args.push("--vpp-libplacebo-tonemapping".to_string());
            args.push(HDR_TONEMAP_ARGS.to_string());
        }

        if let Some(resolution) = &planned.output_resolution {
            args.push("--output-res".to_string());
            args.push(resolution.clone());
        }
        if let Some(resize) = planned.resize {
            args.push("--vpp-resize".to_string());
            args.push(resize.algo.to_string());
            args.push("--vpp-scaler-sharpness".to_string());
            args.push(format!("{:.2}", resize.sharpness));
        }

        if planned.enable_frc {
            args.push("--vpp-frc".to_string());
            args.push(FRC_ARGS.to_string());
        }

        // Reconcile to CFR, as on NVIDIA: source pulls hand container
        // timestamps straight to the encoder, and FRC needs a steady cadence.
        args.push("--avsync".to_string());
        args.push("forcecfr".to_string());

        if self.context.cas_strength > 0.0 {
            args.push("--vpp-unsharp".to_string());
            args.push(format!(
                "radius=3,weight={:.2},threshold=10.0",
                self.context.cas_strength
            ));
        }

        args.extend(planned.hdr_args.iter().cloned());
        if planned.hdr10_metadata {
            args.extend([
                "--max-cll".to_string(),
                HDR10_MAX_CLL.to_string(),
                "--master-display".to_string(),
                HDR10_MASTER_DISPLAY.to_string(),
            ]);
        }
        if planned.ten_bit {
            args.extend([
                "--output-depth".to_string(),
                "10".to_string(),
                "--profile".to_string(),
                "main10".to_string(),
            ]);
        } else {
            args.extend(["--profile".to_string(), "main".to_string()]);
        }

        args.extend([
            "-c".to_string(),
            "hevc".to_string(),
            "--preset".to_string(),
            translate_vceenc_preset(&profile.preset).to_string(),
            "--vbr".to_string(),
            profile.bitrate.to_string(),
            "--max-bitrate".to_string(),
            profile.max_bitrate.to_string(),
            "--bframes".to_string(),
            profile.bframes.to_string(),
            "--pa".to_string(),
            format!("lookahead={}", profile.lookahead),
            "--gop-len".to_string(),
            segment_aligned_gop(output_fps, profile.hls_time).to_string(),
        ]);

        // Paces input frames, before FRC doubles them.
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
            self.context.encoder_backend.clone(),
        ))
    }

    /// Converts the executor-owned operations of the intermediate plan into
    /// VCEEncC options, from their native accelerator bindings.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for shader upscale operations, which the planner only
    /// produces for the Universal executor.
    fn map_intermediate_plan(&self) -> io::Result<PlannedVceEncOptions> {
        debug_assert_eq!(
            self.context.intermediate_plan.executor,
            ExecutorKind::AmdSpecialized
        );

        let mut planned = PlannedVceEncOptions::default();
        let accelerator = |binding: &crate::graph::IntermediateOpBinding| {
            (binding.owner == IntermediateOpOwner::Executor)
                .then(|| binding.accelerator.as_ref().map(|plan| plan.kind))
                .flatten()
        };

        for operation in &self.context.intermediate_plan.operations {
            match operation {
                IntermediateOperation::NormalizeInput(_) => {}
                IntermediateOperation::Resize(resize) => {
                    planned.output_resolution = Some(resize.plan.target_resolution.clone());
                    if accelerator(&resize.binding) == Some(NativeAcceleratorKind::AmdVppResize) {
                        planned.resize = Some(AmfResize::for_quality(
                            resize.plan.quality,
                            self.context.amf_fsr11,
                        ));
                    }
                }
                IntermediateOperation::Interpolate(interpolation) => {
                    if accelerator(&interpolation.binding) == Some(NativeAcceleratorKind::AmdFrc) {
                        planned.enable_frc = true;
                    }
                }
                IntermediateOperation::HdrMetadata(metadata) => {
                    planned.ten_bit |=
                        push_rigaya_hdr_color_args(&mut planned.hdr_args, &metadata.hdr);
                    if metadata.binding.owner == IntermediateOpOwner::Executor {
                        planned.hdr10_metadata = true;
                    }
                }
                IntermediateOperation::HdrTransform(transform) => {
                    planned.ten_bit |=
                        push_rigaya_hdr_color_args(&mut planned.hdr_args, &transform.hdr);
                    if accelerator(&transform.binding) == Some(NativeAcceleratorKind::AmdHdrTonemap)
                    {
                        planned.hdr_tonemap = true;
                    }
                }
                IntermediateOperation::Anime4k2xUpscale(_)
                | IntermediateOperation::Artcnn2xUpscale(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the AMD VCEEncC executor does not render shader upscales",
                    ));
                }
            }
        }

        Ok(planned)
    }
}

impl BackendExecutor for AmdExecutor {
    /// VCEEncC on every attempt. After a VCEEncC startup failure (for
    /// example "Break in task AMFENC" on RDNA 4 / RADV, or an AMF filter the
    /// GPU lacks) the pipeline replaces this executor with the Universal one.
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

#[cfg(test)]
mod tests {
    use super::{translate_vceenc_preset, AmdExecutor, AmdExecutorContext, AmfResize};
    use crate::graph::{
        ColorPrimariesIntent, ExecutionPlan, ExecutorKind, HdrMetadataKind, HdrMetadataPlan,
        HdrPlan, HdrRequest, HdrTransformKind, HdrTransformPlan, InterpolationDecision,
        InterpolationPlan, LatencyMode, MatrixCoefficientsIntent, NativeInterpolationEngine,
        OpExecutionStage, OutputBitDepth, ResizePlan, TransferCharacteristicIntent, VideoOp,
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
    use std::path::PathBuf;

    fn has_subsequence(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }

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

    fn plan(video_ops: Vec<VideoOp>) -> ExecutionPlan {
        ExecutionPlan {
            executor: ExecutorKind::AmdSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops,
        }
    }

    fn stage_context(
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
                runtime_url: "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8".to_string(),
                runtime_headers: HashMap::new(),
                session_headers: HashMap::new(),
                relay: None,
                metadata: Some(SourceMetadata {
                    source_fps: Some(30.0),
                    source_resolution: Some("1920x1080".to_string()),
                    ..SourceMetadata::default()
                }),
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session\\index.m3u8"),
            },
            transport: TransportConfig { input, output },
        }
    }

    fn hls_output() -> FrameTransport {
        FrameTransport::HlsOutput {
            playlist_path: PathBuf::from("session\\index.m3u8"),
            segment_dir: PathBuf::from("session"),
        }
    }

    fn executor(context: &StageRuntimeContext, amf_fsr11: bool) -> AmdExecutor {
        let intermediate_plan = context.execution_plan.to_intermediate_plan();
        AmdExecutor::new(AmdExecutorContext {
            profile: profile(),
            encoder_path: PathBuf::from("VCEEncC64.exe"),
            encoder_backend: "vceenc".to_string(),
            intermediate_plan: intermediate_plan.clone(),
            amf_fsr11,
            denoise: true,
            cas_strength: 0.5,
        })
    }

    fn hdr_op(stage: OpExecutionStage, metadata_stage: OpExecutionStage) -> VideoOp {
        VideoOp::Hdr(HdrPlan {
            request: HdrRequest::TonemapToHdr10,
            output_bit_depth: OutputBitDepth::Bit10,
            color_primaries: ColorPrimariesIntent::Bt2020,
            transfer: TransferCharacteristicIntent::Smpte2084,
            matrix: MatrixCoefficientsIntent::Bt2020Nc,
            metadata: Some(HdrMetadataPlan {
                kind: HdrMetadataKind::Hdr10Static,
                stage: metadata_stage,
            }),
            transform: Some(HdrTransformPlan {
                kind: HdrTransformKind::AmdTonemapToHdr10,
                stage,
            }),
        })
    }

    #[test]
    fn renders_resize_frc_and_hdr_like_the_nvidia_path() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(3),
                    stage: OpExecutionStage::Executor,
                }),
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::native_engine(
                        OpExecutionStage::Executor,
                        NativeInterpolationEngine::AmdFrc,
                    ),
                }),
                hdr_op(OpExecutionStage::Executor, OpExecutionStage::Executor),
            ],
            FrameTransport::SourcePull,
            hls_output(),
        );
        let spec = executor(&context, true)
            .build_executor_spec(&context, 1)
            .unwrap();
        let args = &spec.startup_mode;
        assert_eq!(*args, StartupMode::Buffered, "HLS source pulls buffer");
        let args = &spec.process.args;

        assert!(has_subsequence(args, &["--output-res", "3840x2160"]));
        assert!(has_subsequence(
            args,
            &[
                "--vpp-resize",
                "amf_fsr_11",
                "--vpp-scaler-sharpness",
                "0.75"
            ]
        ));
        assert!(has_subsequence(
            args,
            &["--vpp-frc", "profile=high,search=native"]
        ));
        assert!(has_subsequence(
            args,
            &[
                "--vpp-libplacebo-tonemapping",
                "src_csp=sdr,dst_csp=hdr10,tonemapping_function=spline,inverse_tone_mapping=true,dst_max=1000"
            ]
        ));
        assert!(has_subsequence(args, &["--colorprim", "bt2020"]));
        assert!(has_subsequence(args, &["--transfer", "smpte2084"]));
        assert!(has_subsequence(args, &["--colormatrix", "bt2020nc"]));
        assert!(has_subsequence(args, &["--max-cll", "1000,400"]));
        assert!(args.iter().any(|arg| arg == "--master-display"));
        assert!(has_subsequence(
            args,
            &["--output-depth", "10", "--profile", "main10"]
        ));
        assert!(has_subsequence(args, &["--avsync", "forcecfr"]));
        assert!(has_subsequence(
            args,
            &["--vpp-unsharp", "radius=3,weight=0.50,threshold=10.0"]
        ));
        assert!(args.iter().any(|arg| arg == "--vpp-convolution3d"));
        // 30 fps doubled by FRC: GOP follows the output rate, pacing the input.
        assert!(has_subsequence(args, &["--gop-len", "60"]));
        assert!(has_subsequence(args, &["--max-procfps", "32"]));
        assert!(has_subsequence(
            args,
            &["--mux-option", "hls_delete_threshold:8"]
        ));
        assert!(!args.iter().any(|arg| arg == "--strict-gop"));
        assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
    }

    #[test]
    fn upscale_quality_maps_to_fsr_version_and_sharpness() {
        assert_eq!(
            AmfResize::for_quality(Some(1), true),
            AmfResize {
                algo: "amf_fsr",
                sharpness: 0.5
            }
        );
        assert_eq!(
            AmfResize::for_quality(Some(2), true),
            AmfResize {
                algo: "amf_fsr_11",
                sharpness: 0.5
            }
        );
        assert_eq!(
            AmfResize::for_quality(Some(4), true),
            AmfResize {
                algo: "amf_fsr_11",
                sharpness: 1.0
            }
        );
        // Without FSR 1.1, every level uses FSR 1.0.
        assert_eq!(AmfResize::for_quality(Some(4), false).algo, "amf_fsr");
        assert_eq!(AmfResize::for_quality(None, true).algo, "amf_fsr_11");
    }

    #[test]
    fn crops_direct_source_pulls_to_the_output_aspect() {
        let mut context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "1920x1080".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::SourcePull,
            hls_output(),
        );
        context.source.metadata.as_mut().unwrap().source_resolution = Some("2560x1080".to_string());
        let args = executor(&context, false)
            .build_command(&context, StartupMode::Buffered)
            .unwrap()
            .args;
        assert!(has_subsequence(&args, &["--crop", "320,0,320,0"]));
    }

    #[test]
    fn staged_input_reads_nut_and_skips_source_only_options() {
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
                    decision: InterpolationDecision::rife(OpExecutionStage::Preprocess),
                }),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let spec = executor(&context, false)
            .build_executor_spec(&context, 1)
            .unwrap();
        let args = &spec.process.args;

        assert!(has_subsequence(args, &["--avsw", "-i", "-"]));
        assert!(!args.iter().any(|arg| arg == "--crop"));
        assert!(!args.iter().any(|arg| arg == "--vpp-convolution3d"));
        // RIFE runs upstream; VCEEncC neither interpolates nor paces a pipe.
        assert!(!args.iter().any(|arg| arg == "--vpp-frc"));
        assert!(!args.iter().any(|arg| arg == "--max-procfps"));
        assert!(has_subsequence(args, &["--profile", "main"]));
        assert!(has_subsequence(args, &["-f", "mpegts"]) || args.iter().any(|arg| arg == "-"));
    }

    #[test]
    fn hdr_without_native_tonemap_keeps_signaling_but_skips_libplacebo() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                hdr_op(OpExecutionStage::Preprocess, OpExecutionStage::Packager),
            ],
            FrameTransport::StdoutPipe,
            FrameTransport::StdoutPipe,
        );
        let args = executor(&context, false)
            .build_command(&context, StartupMode::LowLatency)
            .unwrap()
            .args;
        assert!(!args.iter().any(|arg| arg == "--vpp-libplacebo-tonemapping"));
        assert!(!args.iter().any(|arg| arg == "--max-cll"));
        assert!(has_subsequence(args.as_slice(), &["--profile", "main10"]));
    }

    #[test]
    fn rejects_shader_upscales() {
        let context = stage_context(
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Artcnn2xUpscale(crate::graph::Artcnn2xPlan {
                    target_resolution: "3840x2160".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
            ],
            FrameTransport::StdoutPipe,
            hls_output(),
        );
        assert!(executor(&context, false)
            .build_command(&context, StartupMode::LowLatency)
            .is_err());
    }

    #[test]
    fn translates_nvenc_presets() {
        assert_eq!(translate_vceenc_preset("p1"), "fast");
        assert_eq!(translate_vceenc_preset("p4"), "balanced");
        assert_eq!(translate_vceenc_preset("p7"), "slow");
    }
}
