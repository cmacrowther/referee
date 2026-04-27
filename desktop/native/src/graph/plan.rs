use serde::{Deserialize, Serialize};

use super::request::{HdrRequest, LatencyMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HdrTransformKind {
    Passthrough10Bit,
    TonemapToHdr10,
    NvidiaTrueHdrTonemapToHdr10,
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
    NativeBackend,
    PortableFallback,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterpolationUnsupportedReason {
    SourceAlreadyAtOrAboveTargetFps,
    PortableFallbackNotImplemented,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterpolationDecision {
    pub realization: InterpolationRealization,
    pub stage: Option<OpExecutionStage>,
    pub unsupported_reason: Option<InterpolationUnsupportedReason>,
}

impl InterpolationDecision {
    /// Create an interpolation decision that uses the native backend and assigns it to the given execution stage.
    ///
    /// The created decision is enabled and has no unsupported reason recorded.
    ///
    /// # Examples
    ///
    pub fn native_backend(stage: OpExecutionStage) -> Self {
        Self {
            realization: InterpolationRealization::NativeBackend,
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
