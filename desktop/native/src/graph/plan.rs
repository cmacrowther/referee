use serde::{Deserialize, Serialize};

use super::request::{HdrRequest, LatencyMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HdrTransformKind {
    Passthrough10Bit,
    TonemapToHdr10,
    NvidiaTrueHdrTonemapToHdr10,
    /// SDR to HDR10 with libplacebo inverse tone mapping inside VCEEncC.
    AmdTonemapToHdr10,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HdrMetadataKind {
    Hdr10Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpExecutionStage {
    Preprocess,
    Executor,
    Packager,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputBitDepth {
    Bit8,
    Bit10,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorPrimariesIntent {
    PreserveSource,
    Bt2020,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferCharacteristicIntent {
    PreserveSource,
    Smpte2084,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatrixCoefficientsIntent {
    PreserveSource,
    Bt2020Nc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizePlan {
    pub target_resolution: String,
    pub quality: Option<u8>,
    pub stage: OpExecutionStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterpolationRealization {
    NativeBackend(NativeInterpolationEngine),
    /// FFmpeg `minterpolate` in the preprocess stage.
    PortableFallback,
    /// RIFE (`rife-worker`) in the preprocess stage.
    Rife,
    Disabled,
}

/// The executor-native interpolator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NativeInterpolationEngine {
    /// NVEncC optical flow FRUC: any target frame rate.
    #[default]
    NvidiaFruc,
    /// NVEncC NVIDIA frame generation (Maxine): whole-number multiples.
    NvidiaFramegen,
    /// VCEEncC AMF FRC (AMD Fluid Motion): doubles the frame rate.
    AmdFrc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterpolationUnsupportedReason {
    SourceAlreadyAtOrAboveTargetFps,
    PortableFallbackNotImplemented,
    /// The plan's native engine (AMF FRC) is not on the executor rendering it,
    /// as in the AMD executor's FFmpeg fallback.
    NativeEngineUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterpolationDecision {
    pub realization: InterpolationRealization,
    pub stage: Option<OpExecutionStage>,
    pub unsupported_reason: Option<InterpolationUnsupportedReason>,
}

impl InterpolationDecision {
    /// Create an interpolation decision that uses the native backend's optical
    /// flow (FRUC) and assigns it to the given execution stage.
    ///
    /// The created decision is enabled and has no unsupported reason recorded.
    ///
    /// # Examples
    ///
    pub fn native_backend(stage: OpExecutionStage) -> Self {
        Self::native_engine(stage, NativeInterpolationEngine::NvidiaFruc)
    }

    /// Create an enabled interpolation decision that uses `engine` at `stage`.
    pub fn native_engine(stage: OpExecutionStage, engine: NativeInterpolationEngine) -> Self {
        Self {
            realization: InterpolationRealization::NativeBackend(engine),
            stage: Some(stage),
            unsupported_reason: None,
        }
    }

    /// Creates an interpolation decision that requests the portable fallback implementation at the given execution stage.
    ///
    /// The resulting `InterpolationDecision` has `realization` set to `InterpolationRealization::PortableFallback`,
    /// `stage` set to the provided `stage`, and `unsupported_reason` set to `None`.
    ///
    /// # Examples
    ///
    pub fn portable_fallback(stage: OpExecutionStage) -> Self {
        Self {
            realization: InterpolationRealization::PortableFallback,
            stage: Some(stage),
            unsupported_reason: None,
        }
    }

    /// Create an enabled interpolation decision that runs RIFE at `stage`.
    pub fn rife(stage: OpExecutionStage) -> Self {
        Self {
            realization: InterpolationRealization::Rife,
            stage: Some(stage),
            unsupported_reason: None,
        }
    }

    /// Constructs an `InterpolationDecision` that uses the portable fallback, records the execution `stage`, and sets an `unsupported_reason`.
    ///
    /// # Examples
    ///
    ///
    /// # Returns
    ///
    /// An `InterpolationDecision` with `realization` set to `PortableFallback`, `stage` set to the provided `stage`, and `unsupported_reason` set to the provided `reason`.
    pub fn portable_fallback_with_gap(
        stage: OpExecutionStage,
        reason: InterpolationUnsupportedReason,
    ) -> Self {
        Self {
            realization: InterpolationRealization::PortableFallback,
            stage: Some(stage),
            unsupported_reason: Some(reason),
        }
    }

    /// Create an `InterpolationDecision` that disables interpolation for the specified reason.
    ///
    /// The resulting decision has `realization` set to `Disabled`, `stage` set to `None`,
    /// and `unsupported_reason` set to the provided `reason`.
    ///
    /// # Examples
    ///
    pub fn disabled(reason: InterpolationUnsupportedReason) -> Self {
        Self {
            realization: InterpolationRealization::Disabled,
            stage: None,
            unsupported_reason: Some(reason),
        }
    }

    /// Indicates whether interpolation is enabled.
    ///
    /// # Returns
    ///
    /// `true` if interpolation is enabled (the realization is not `Disabled`), `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn is_enabled(&self) -> bool {
        !matches!(self.realization, InterpolationRealization::Disabled)
    }

    /// Checks whether the interpolation decision can be executed.
    ///
    /// The decision is executable when it is enabled and there is no recorded unsupported reason.
    ///
    /// # Returns
    ///
    /// `true` if the decision is enabled and has no unsupported reason, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn is_executable(&self) -> bool {
        self.is_enabled() && self.unsupported_reason.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterpolationPlan {
    pub target_fps: u32,
    pub decision: InterpolationDecision,
}

impl InterpolationPlan {
    pub fn stage(&self) -> Option<OpExecutionStage> {
        self.decision.stage
    }

    /// Reports whether interpolation is enabled for this plan.
    ///
    /// `true` if interpolation is enabled, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn is_enabled(&self) -> bool {
        self.decision.is_enabled()
    }

    /// Reports whether this interpolation plan can actually be executed.
    ///
    /// Returns `true` if the plan is enabled and does not record an unsupported reason, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn is_executable(&self) -> bool {
        self.decision.is_executable()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HdrMetadataPlan {
    pub kind: HdrMetadataKind,
    pub stage: OpExecutionStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HdrTransformPlan {
    pub kind: HdrTransformKind,
    pub stage: OpExecutionStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HdrPlan {
    pub request: HdrRequest,
    pub output_bit_depth: OutputBitDepth,
    pub color_primaries: ColorPrimariesIntent,
    pub transfer: TransferCharacteristicIntent,
    pub matrix: MatrixCoefficientsIntent,
    /// Desired static HDR metadata in the encoded output. The chosen stage is
    /// the planner's preferred owner, but executors may still realize compatible
    /// subsets of the intent when that helps preserve partial HDR success.
    pub metadata: Option<HdrMetadataPlan>,
    /// Desired transform/synthesis work such as 10-bit passthrough or HDR
    /// tonemapping. This stays separate from metadata so unsupported transforms
    /// do not erase output-format or metadata intent.
    pub transform: Option<HdrTransformPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anime4k2xPlan {
    pub target_resolution: String,
    pub stage: OpExecutionStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artcnn2xPlan {
    pub target_resolution: String,
    pub stage: OpExecutionStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoOp {
    NormalizeInput,
    Resize(ResizePlan),
    Interpolate(InterpolationPlan),
    Hdr(HdrPlan),
    Anime4k2xUpscale(Anime4k2xPlan),
    Artcnn2xUpscale(Artcnn2xPlan),
}

impl VideoOp {
    pub fn requires_stage(&self, stage: OpExecutionStage) -> bool {
        match self {
            Self::NormalizeInput => false,
            Self::Resize(resize) => resize.stage == stage,
            Self::Interpolate(interpolation) => interpolation.stage() == Some(stage),
            Self::Hdr(hdr) => {
                hdr.metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.stage == stage)
                    || hdr
                        .transform
                        .as_ref()
                        .is_some_and(|transform| transform.stage == stage)
            }
            Self::Anime4k2xUpscale(plan) => plan.stage == stage,
            Self::Artcnn2xUpscale(plan) => plan.stage == stage,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutorKind {
    #[serde(alias = "nvidia")]
    NvidiaSpecialized,
    /// The cross-platform FFmpeg/libplacebo executor.
    ///
    /// The `"amd"` alias is a legacy wire-format name kept for backwards compatibility
    /// with plans serialised before the AMD-native executor seam was separated from the
    /// Universal path. New code should produce `"universal"` on the wire; the alias
    /// only exists to deserialise older persisted plans without a migration step.
    #[serde(alias = "amd")]
    Universal,
    Cpu,
    /// VCEEncC on AMD GPUs: AMF FSR resize, AMF FRC (Fluid Motion) frame
    /// doubling, and libplacebo HDR, the AMD counterpart of
    /// `NvidiaSpecialized`.
    AmdSpecialized,
}

impl ExecutorKind {
    /// The vendor-native executors (NVEncC, VCEEncC).
    pub fn is_specialized(self) -> bool {
        matches!(self, Self::NvidiaSpecialized | Self::AmdSpecialized)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlan {
    pub executor: ExecutorKind,
    pub latency_mode: LatencyMode,
    pub requires_local_hls_relay: bool,
    pub video_ops: Vec<VideoOp>,
}

impl ExecutionPlan {
    pub fn interpolation_plan(&self) -> Option<&InterpolationPlan> {
        self.video_ops.iter().find_map(|video_op| match video_op {
            VideoOp::Interpolate(interpolation) => Some(interpolation),
            _ => None,
        })
    }

    pub fn requires_stage(&self, stage: OpExecutionStage) -> bool {
        self.video_ops
            .iter()
            .any(|video_op| video_op.requires_stage(stage))
    }

    pub fn requires_preprocess_stage(&self) -> bool {
        self.requires_stage(OpExecutionStage::Preprocess)
    }

    /// The plan the Universal FFmpeg executor renders when the AMD executor
    /// falls back: native-only interpolation (AMF FRC) is disabled, so the
    /// output rate and GOP follow the source. Everything else is unchanged.
    pub fn for_universal_fallback(&self) -> Self {
        let mut plan = self.clone();
        plan.executor = ExecutorKind::Universal;
        for video_op in &mut plan.video_ops {
            if let VideoOp::Interpolate(interpolation) = video_op {
                if matches!(
                    interpolation.decision.realization,
                    InterpolationRealization::NativeBackend(_)
                ) {
                    interpolation.decision = InterpolationDecision::disabled(
                        InterpolationUnsupportedReason::NativeEngineUnavailable,
                    );
                }
            }
        }
        plan
    }

    /// Whether the plan interpolates with RIFE.
    pub fn uses_rife(&self) -> bool {
        self.interpolation_plan().is_some_and(|interpolation| {
            interpolation.is_executable()
                && interpolation.decision.realization == InterpolationRealization::Rife
        })
    }

    /// The engine generating frames, for display: `"NVIDIA FRUC"`, `"NVIDIA
    /// Framegen"`, `"RIFE"`, or `"Portable"`. `None` when frame generation
    /// does not run.
    pub fn framegen_label(&self) -> Option<&'static str> {
        let interpolation = self
            .interpolation_plan()
            .filter(|interpolation| interpolation.is_executable())?;
        match interpolation.decision.realization {
            InterpolationRealization::NativeBackend(NativeInterpolationEngine::NvidiaFruc) => {
                Some("NVIDIA FRUC")
            }
            InterpolationRealization::NativeBackend(NativeInterpolationEngine::NvidiaFramegen) => {
                Some("NVIDIA Framegen")
            }
            InterpolationRealization::NativeBackend(NativeInterpolationEngine::AmdFrc) => {
                Some("AMD Fluid Motion")
            }
            InterpolationRealization::Rife => Some("RIFE"),
            InterpolationRealization::PortableFallback => Some("Portable"),
            InterpolationRealization::Disabled => None,
        }
    }

    /// Whether the plan includes interpolation that will actually run.
    pub fn has_executable_interpolation(&self) -> bool {
        self.interpolation_plan()
            .is_some_and(|interpolation| interpolation.is_executable())
    }

    /// The frame rate the executor emits: the interpolation target when
    /// interpolation runs, otherwise the source rate. `None` when interpolation
    /// does not run and the source frame rate is unknown or not positive.
    pub fn output_frame_rate(&self, source_fps: Option<f64>) -> Option<f64> {
        let target = match self.interpolation_plan() {
            Some(interpolation) if interpolation.is_executable() => interpolation.target_fps as f64,
            _ => source_fps.filter(|fps| *fps > 0.0)?,
        };
        Some(round_frame_rate(target))
    }
}

impl OutputBitDepth {
    /// Planar YUV pixel format for this bit depth.
    pub fn planar_yuv_format(self) -> &'static str {
        match self {
            Self::Bit8 => "yuv420p",
            Self::Bit10 => "yuv420p10le",
        }
    }
}

/// Rounds a frame rate to two decimal places (59.94006 -> 59.94).
pub fn round_frame_rate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Parses a `"WxH"` resolution string. Returns `None` unless both dimensions
/// are positive integers.
pub fn parse_resolution(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::{
        ColorPrimariesIntent, ExecutionPlan, ExecutorKind, HdrMetadataKind, HdrMetadataPlan,
        HdrPlan, HdrRequest, HdrTransformKind, HdrTransformPlan, LatencyMode,
        MatrixCoefficientsIntent, OpExecutionStage, OutputBitDepth, ResizePlan,
        TransferCharacteristicIntent, VideoOp,
    };

    #[test]
    fn execution_plan_detects_preprocess_stage_requirements_across_video_ops() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
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
                    metadata: Some(HdrMetadataPlan {
                        kind: HdrMetadataKind::Hdr10Static,
                        stage: OpExecutionStage::Packager,
                    }),
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::TonemapToHdr10,
                        stage: OpExecutionStage::Preprocess,
                    }),
                }),
            ],
        };

        assert!(plan.requires_preprocess_stage());
        assert!(plan.requires_stage(OpExecutionStage::Packager));
        assert!(!plan.requires_stage(OpExecutionStage::Executor));
    }

    #[test]
    fn execution_plan_ignores_normalize_only_paths_when_checking_preprocess_stage() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        };

        assert!(!plan.requires_preprocess_stage());
    }
}

#[cfg(test)]
mod universal_fallback_tests {
    use super::{
        ExecutionPlan, ExecutorKind, InterpolationDecision, InterpolationPlan,
        InterpolationRealization, NativeInterpolationEngine, OpExecutionStage, VideoOp,
    };
    use crate::graph::LatencyMode;

    fn amd_plan(decision: InterpolationDecision) -> ExecutionPlan {
        ExecutionPlan {
            executor: ExecutorKind::AmdSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision,
                }),
            ],
        }
    }

    #[test]
    fn drops_amd_frc_and_returns_to_the_source_rate() {
        let fallback = amd_plan(InterpolationDecision::native_engine(
            OpExecutionStage::Executor,
            NativeInterpolationEngine::AmdFrc,
        ))
        .for_universal_fallback();

        assert_eq!(fallback.executor, ExecutorKind::Universal);
        assert!(!fallback.has_executable_interpolation());
        assert_eq!(fallback.framegen_label(), None);
        assert_eq!(fallback.output_frame_rate(Some(30.0)), Some(30.0));
    }

    #[test]
    fn keeps_preprocess_interpolation_that_ffmpeg_can_run() {
        let fallback = amd_plan(InterpolationDecision::rife(OpExecutionStage::Preprocess))
            .for_universal_fallback();

        assert_eq!(fallback.executor, ExecutorKind::Universal);
        assert_eq!(
            fallback
                .interpolation_plan()
                .map(|plan| plan.decision.realization.clone()),
            Some(InterpolationRealization::Rife)
        );
        assert_eq!(fallback.output_frame_rate(Some(30.0)), Some(60.0));
    }
}

#[cfg(test)]
mod frame_rate_tests {
    use super::{
        ExecutionPlan, ExecutorKind, InterpolationDecision, InterpolationPlan,
        InterpolationUnsupportedReason, OpExecutionStage, VideoOp,
    };
    use crate::graph::LatencyMode;

    #[test]
    fn output_frame_rate_returns_native_interpolation_target_fps() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision: InterpolationDecision::native_backend(OpExecutionStage::Executor),
            })],
        };

        assert_eq!(plan.output_frame_rate(Some(24.0)), Some(60.0));
        // The target is known even when the source rate is not.
        assert_eq!(plan.output_frame_rate(None), Some(60.0));
    }

    #[test]
    fn output_frame_rate_keeps_source_fps_for_portable_interpolation_gap() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision: InterpolationDecision::portable_fallback_with_gap(
                    OpExecutionStage::Preprocess,
                    InterpolationUnsupportedReason::PortableFallbackNotImplemented,
                ),
            })],
        };

        assert!(!plan.has_executable_interpolation());
        assert_eq!(plan.output_frame_rate(Some(30.0)), Some(30.0));
    }

    #[test]
    fn output_frame_rate_passes_through_source_fps_when_interpolation_is_off() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        };

        assert_eq!(plan.output_frame_rate(Some(24.0)), Some(24.0));
        assert_eq!(plan.output_frame_rate(None), None);
    }

    #[test]
    fn output_frame_rate_rounds_and_rejects_non_positive_source_rates() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        };

        assert_eq!(plan.output_frame_rate(Some(60000.0 / 1001.0)), Some(59.94));
        assert_eq!(plan.output_frame_rate(Some(0.0)), None);
    }

    #[test]
    fn parse_resolution_requires_two_positive_dimensions() {
        assert_eq!(super::parse_resolution("1920x1080"), Some((1920, 1080)));
        assert_eq!(super::parse_resolution(" 1280 x 720 "), Some((1280, 720)));
        assert_eq!(super::parse_resolution("0x1080"), None);
        assert_eq!(super::parse_resolution("1920"), None);
    }
}
