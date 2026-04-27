use serde::{Deserialize, Serialize};

use super::plan::{
    Anime4k2xPlan, Artcnn2xPlan, ExecutionPlan, ExecutorKind, HdrMetadataPlan, HdrPlan,
    HdrTransformPlan, InterpolationPlan, InterpolationRealization, InterpolationUnsupportedReason,
    OpExecutionStage, ResizePlan, VideoOp,
};
use super::request::LatencyMode;

/// Backend-agnostic plan shape that sits between graph planning and backend
/// command rendering.
///
/// The existing `ExecutionPlan` remains the source of truth for graph planning
/// decisions. This intermediate layer preserves those decisions while making
/// the next runtime seam explicit:
/// - which work is shared portable processing,
/// - which work remains executor-owned,
/// - which executor-owned work can use an optional native accelerator,
/// - which requested work is still unresolved/deferred today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntermediateExecutionPlan {
    pub executor: ExecutorKind,
    pub latency_mode: LatencyMode,
    pub requires_local_hls_relay: bool,
    pub operations: Vec<IntermediateOperation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntermediateOpOwner {
    Normalizer,
    SharedPreprocess,
    Executor,
    Packager,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeAcceleratorKind {
    NvidiaNgxVsr,
    NvidiaFruc,
    NvidiaTrueHdr,
    AmdVppResize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAcceleratorPlan {
    pub kind: NativeAcceleratorKind,
    pub fallback_owner: IntermediateOpOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnresolvedOperationReason {
    InterpolationUnsupported(InterpolationUnsupportedReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntermediateOpBinding {
    pub owner: IntermediateOpOwner,
    pub conceptual_owner: IntermediateOpOwner,
    pub accelerator: Option<NativeAcceleratorPlan>,
    pub unresolved_reason: Option<UnresolvedOperationReason>,
}

impl IntermediateOpBinding {
    /// Returns whether this binding has an unresolved reason.
    ///
    /// The method reports if the binding was marked as unresolved (i.e., `unresolved_reason` is `Some`).
    ///
    /// # Returns
    ///
    /// `true` if `unresolved_reason` is present, `false` otherwise.
    pub fn is_unresolved(&self) -> bool {
        self.unresolved_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizeInputOp {
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeOp {
    pub plan: ResizePlan,
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterpolateOp {
    pub plan: InterpolationPlan,
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HdrMetadataOp {
    pub hdr: HdrPlan,
    pub plan: HdrMetadataPlan,
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HdrTransformOp {
    pub hdr: HdrPlan,
    pub plan: HdrTransformPlan,
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anime4k2xUpscaleOp {
    pub plan: Anime4k2xPlan,
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artcnn2xUpscaleOp {
    pub plan: Artcnn2xPlan,
    pub binding: IntermediateOpBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SharedNormalizationOperation {
    NormalizeInput(NormalizeInputOp),
}

impl SharedNormalizationOperation {
    /// Accesses the inner operation's binding.
    ///
    /// Returns a reference to the `IntermediateOpBinding` associated with this
    /// shared normalization operation.
    ///
    /// # Returns
    ///
    /// `&IntermediateOpBinding` reference to the operation's binding.
    pub fn binding(&self) -> &IntermediateOpBinding {
        match self {
            Self::NormalizeInput(op) => &op.binding,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedNormalizationPlan {
    pub operations: Vec<SharedNormalizationOperation>,
}

impl SharedNormalizationPlan {
    /// Reports whether the normalization stage is required for this plan.
    ///
    /// # Returns
    ///
    /// `true` if at least one normalization operation is present, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn requires_stage(&self) -> bool {
        !self.operations.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SharedPreprocessOperation {
    Resize(ResizeOp),
    Interpolate(InterpolateOp),
    HdrTransform(HdrTransformOp),
    Anime4k2xUpscale(Anime4k2xUpscaleOp),
    Artcnn2xUpscale(Artcnn2xUpscaleOp),
}

impl SharedPreprocessOperation {
    /// Returns a reference to the operation's inner binding.
    ///
    /// # Examples
    ///
    pub fn binding(&self) -> &IntermediateOpBinding {
        match self {
            Self::Resize(op) => &op.binding,
            Self::Interpolate(op) => &op.binding,
            Self::HdrTransform(op) => &op.binding,
            Self::Anime4k2xUpscale(op) => &op.binding,
            Self::Artcnn2xUpscale(op) => &op.binding,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SharedPreprocessExecutionMode {
    PreprocessorOwned,
    TemporaryExecutorFallback,
    DeferredUntilPortableRenderer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPreprocessOperationPlan {
    pub operation: SharedPreprocessOperation,
    pub execution_mode: SharedPreprocessExecutionMode,
}

impl SharedPreprocessOperationPlan {
    /// Accesses the inner operation's binding.
    ///
    /// # Returns
    ///
    /// A reference to the inner operation's `IntermediateOpBinding`.
    ///
    /// # Examples
    ///
    pub fn binding(&self) -> &IntermediateOpBinding {
        self.operation.binding()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPreprocessPlan {
    pub operations: Vec<SharedPreprocessOperationPlan>,
}

impl SharedPreprocessPlan {
    /// Returns whether any shared-preprocess operation requires running in the preprocessor stage.
    ///
    /// Checks if the plan contains at least one operation with execution mode
    /// `SharedPreprocessExecutionMode::PreprocessorOwned`.
    ///
    /// # Returns
    ///
    /// `true` if any operation's execution mode is `PreprocessorOwned`, `false` otherwise.
    pub fn requires_stage(&self) -> bool {
        self.operations.iter().any(|operation| {
            operation.execution_mode == SharedPreprocessExecutionMode::PreprocessorOwned
        })
    }

    /// Returns whether the plan contains any operation that requires a temporary executor fallback.
    ///
    /// The method checks for any `SharedPreprocessOperationPlan` whose `execution_mode` is
    /// `SharedPreprocessExecutionMode::TemporaryExecutorFallback`.
    ///
    /// # Returns
    ///
    /// `true` if at least one operation has `SharedPreprocessExecutionMode::TemporaryExecutorFallback`, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn has_temporary_executor_fallbacks(&self) -> bool {
        self.operations.iter().any(|operation| {
            operation.execution_mode == SharedPreprocessExecutionMode::TemporaryExecutorFallback
        })
    }

    /// Checks whether the plan contains any operation that is deferred until a portable renderer
    /// because its portable fallback is unimplemented.
    ///
    /// # Examples
    ///
    pub fn has_unimplemented_portable_operations(&self) -> bool {
        self.operations.iter().any(|operation| {
            operation.execution_mode == SharedPreprocessExecutionMode::DeferredUntilPortableRenderer
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntermediateOperation {
    NormalizeInput(NormalizeInputOp),
    Resize(ResizeOp),
    Interpolate(InterpolateOp),
    HdrMetadata(HdrMetadataOp),
    HdrTransform(HdrTransformOp),
    Anime4k2xUpscale(Anime4k2xUpscaleOp),
    Artcnn2xUpscale(Artcnn2xUpscaleOp),
}

impl IntermediateOperation {
    /// Get the binding associated with this intermediate operation.
    ///
    /// The returned reference points to the `IntermediateOpBinding` describing ownership,
    /// accelerator hints, and any unresolved reason for the operation.
    ///
    /// # Examples
    ///
    pub fn binding(&self) -> &IntermediateOpBinding {
        match self {
            Self::NormalizeInput(op) => &op.binding,
            Self::Resize(op) => &op.binding,
            Self::Interpolate(op) => &op.binding,
            Self::HdrMetadata(op) => &op.binding,
            Self::HdrTransform(op) => &op.binding,
            Self::Anime4k2xUpscale(op) => &op.binding,
            Self::Artcnn2xUpscale(op) => &op.binding,
        }
    }
}

impl IntermediateExecutionPlan {
    /// Builds an IntermediateExecutionPlan by converting each VideoOp in an ExecutionPlan
    /// into the corresponding IntermediateOperation with its computed binding.
    ///
    /// # Examples
    ///
    pub fn from_execution_plan(plan: &ExecutionPlan) -> Self {
        let mut operations = Vec::new();

        for video_op in &plan.video_ops {
            match video_op {
                VideoOp::NormalizeInput => {
                    operations.push(IntermediateOperation::NormalizeInput(NormalizeInputOp {
                        binding: IntermediateOpBinding {
                            owner: IntermediateOpOwner::Normalizer,
                            conceptual_owner: IntermediateOpOwner::Normalizer,
                            accelerator: None,
                            unresolved_reason: None,
                        },
                    }));
                }
                VideoOp::Resize(resize) => {
                    operations.push(IntermediateOperation::Resize(ResizeOp {
                        plan: resize.clone(),
                        binding: resize_binding(plan.executor, resize),
                    }));
                }
                VideoOp::Interpolate(interpolation) => {
                    operations.push(IntermediateOperation::Interpolate(InterpolateOp {
                        plan: interpolation.clone(),
                        binding: interpolation_binding(plan.executor, interpolation),
                    }));
                }
                VideoOp::Hdr(hdr) => {
                    if let Some(metadata) = hdr.metadata.clone() {
                        operations.push(IntermediateOperation::HdrMetadata(HdrMetadataOp {
                            hdr: hdr.clone(),
                            plan: metadata.clone(),
                            binding: binding_for_stage(metadata.stage),
                        }));
                    }

                    if let Some(transform) = hdr.transform.clone() {
                        operations.push(IntermediateOperation::HdrTransform(HdrTransformOp {
                            hdr: hdr.clone(),
                            plan: transform.clone(),
                            binding: hdr_transform_binding(plan.executor, &transform),
                        }));
                    }
                }
                VideoOp::Anime4k2xUpscale(shader) => {
                    operations.push(IntermediateOperation::Anime4k2xUpscale(
                        Anime4k2xUpscaleOp {
                            plan: shader.clone(),
                            binding: shader_binding(shader.stage),
                        },
                    ));
                }
                VideoOp::Artcnn2xUpscale(shader) => {
                    operations.push(IntermediateOperation::Artcnn2xUpscale(Artcnn2xUpscaleOp {
                        plan: shader.clone(),
                        binding: shader_binding(shader.stage),
                    }));
                }
            }
        }

        Self {
            executor: plan.executor,
            latency_mode: plan.latency_mode,
            requires_local_hls_relay: plan.requires_local_hls_relay,
            operations,
        }
    }

    /// Checks whether this intermediate execution plan requires a shared preprocess stage.
    ///
    /// Returns `true` if any operation in the plan is owned by `SharedPreprocess`, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn requires_shared_preprocess(&self) -> bool {
        self.operations
            .iter()
            .any(|operation| operation.binding().owner == IntermediateOpOwner::SharedPreprocess)
    }

    /// Detects whether the plan conceptually assigns any operation to shared preprocess.
    ///
    /// # Returns
    ///
    /// `true` if any operation's `conceptual_owner` is `IntermediateOpOwner::SharedPreprocess`, `false` otherwise.
    ///
    /// # Examples
    ///
    pub fn conceptually_uses_shared_preprocess(&self) -> bool {
        self.operations.iter().any(|operation| {
            operation.binding().conceptual_owner == IntermediateOpOwner::SharedPreprocess
        })
    }

    /// Builds a SharedNormalizationPlan containing only the normalization operations from this plan,
    /// preserving each operation's full binding.
    ///
    /// # Examples
    ///
    pub fn normalization_plan(&self) -> SharedNormalizationPlan {
        SharedNormalizationPlan {
            operations: self
                .operations
                .iter()
                .filter_map(|operation| match operation {
                    IntermediateOperation::NormalizeInput(op) => {
                        Some(SharedNormalizationOperation::NormalizeInput(op.clone()))
                    }
                    _ => None,
                })
                .collect(),
        }
    }

    /// Produces a plan describing the operations that should be handled by the shared preprocess stage.
    ///
    /// Filters this execution plan's operations and collects those that are conceptually owned by
    /// shared preprocess into a `SharedPreprocessPlan`.
    ///
    /// # Examples
    ///
    pub fn shared_preprocess_plan(&self) -> SharedPreprocessPlan {
        SharedPreprocessPlan {
            operations: self
                .operations
                .iter()
                .filter_map(shared_preprocess_operation_plan)
                .collect(),
        }
    }

    /// Promotes eligible executor-owned resize operations to AMD-native resize bindings when AMD upscaling is supported.
    ///
    /// This refinement converts executor-stage resize operations that are currently
    /// owned by the executor (and have no native accelerator assigned) into
    /// executor-native work by setting their conceptual owner to `Executor` and
    /// attaching an `AmdVppResize` native accelerator with a `SharedPreprocess` fallback.
    /// The method is a no-op unless `native_upscale_supported` is `true` and the plan's
    /// `executor` is `ExecutorKind::Universal`.
    ///
    /// # Examples
    ///
    pub fn with_amd_native_resize_bindings(mut self, native_upscale_supported: bool) -> Self {
        if !native_upscale_supported || self.executor != ExecutorKind::Universal {
            return self;
        }

        for operation in &mut self.operations {
            let IntermediateOperation::Resize(resize) = operation else {
                continue;
            };

            if resize.binding.owner != IntermediateOpOwner::Executor
                || resize.binding.accelerator.is_some()
            {
                continue;
            }

            resize.binding.conceptual_owner = IntermediateOpOwner::Executor;
            resize.binding.accelerator = Some(NativeAcceleratorPlan {
                kind: NativeAcceleratorKind::AmdVppResize,
                fallback_owner: IntermediateOpOwner::SharedPreprocess,
            });
        }

        self
    }
}

impl ExecutionPlan {
    /// Create an IntermediateExecutionPlan representing this ExecutionPlan.
    ///
    /// The returned plan is a backend-agnostic, serializable representation of the execution
    /// steps suitable for backend-specific refinement and shared-preprocess extraction.
    ///
    /// # Examples
    ///
    pub fn to_intermediate_plan(&self) -> IntermediateExecutionPlan {
        IntermediateExecutionPlan::from_execution_plan(self)
    }
}

/// Create an `IntermediateOpBinding` corresponding to an `OpExecutionStage`.
///
/// The returned binding has `owner` and `conceptual_owner` set according to `stage`,
/// and `accelerator` and `unresolved_reason` set to `None`.
///
/// # Examples
///
fn binding_for_stage(stage: OpExecutionStage) -> IntermediateOpBinding {
    IntermediateOpBinding {
        owner: match stage {
            OpExecutionStage::Preprocess => IntermediateOpOwner::SharedPreprocess,
            OpExecutionStage::Executor => IntermediateOpOwner::Executor,
            OpExecutionStage::Packager => IntermediateOpOwner::Packager,
            OpExecutionStage::Deferred => IntermediateOpOwner::Deferred,
        },
        conceptual_owner: match stage {
            OpExecutionStage::Preprocess => IntermediateOpOwner::SharedPreprocess,
            OpExecutionStage::Executor => IntermediateOpOwner::Executor,
            OpExecutionStage::Packager => IntermediateOpOwner::Packager,
            OpExecutionStage::Deferred => IntermediateOpOwner::Deferred,
        },
        accelerator: None,
        unresolved_reason: None,
    }
}

/// Builds an `IntermediateOpBinding` for a resize operation based on the operation's
/// execution stage and the active executor.
///
/// When the resize is scheduled for the `Executor` stage and the executor is
/// `ExecutorKind::NvidiaSpecialized` with a specified `quality`, the binding will
/// include an `NvidiaNgxVsr` native accelerator and set its fallback owner to
/// `SharedPreprocess`. If the resize is scheduled for the `Executor` stage but
/// does not meet the Nvidia/quality criteria, the binding's conceptual owner is
/// set to `SharedPreprocess`.
///
/// # Returns
///
/// An `IntermediateOpBinding` configured for the given `resize` according to the
/// executor and stage rules described above.
///
/// # Examples
///
fn resize_binding(executor: ExecutorKind, resize: &ResizePlan) -> IntermediateOpBinding {
    let mut binding = binding_for_stage(resize.stage);

    if resize.stage == OpExecutionStage::Executor
        && executor == ExecutorKind::NvidiaSpecialized
        && resize.quality.is_some()
    {
        binding.accelerator = Some(NativeAcceleratorPlan {
            kind: NativeAcceleratorKind::NvidiaNgxVsr,
            fallback_owner: IntermediateOpOwner::SharedPreprocess,
        });
    } else if resize.stage == OpExecutionStage::Executor {
        binding.conceptual_owner = IntermediateOpOwner::SharedPreprocess;
    }

    binding
}

/// Constructs an IntermediateOpBinding for an interpolation plan based on its realization and the executor.
///
/// - For a disabled realization, the binding is owned by `Deferred`; its `conceptual_owner` becomes
///   `SharedPreprocess` only when the unsupported reason is `PortableFallbackNotImplemented`. Any
///   unsupported reason is recorded in `unresolved_reason`.
/// - For a native-backend realization, the binding is derived from the interpolation's execution
///   stage; when the executor is `NvidiaSpecialized` and the stage is `Executor`, a
///   `NvidiaFruc` native accelerator is attached with a fallback owner of `SharedPreprocess`.
/// - For a portable-fallback realization, an unsupported reason produces a deferred binding with
///   `conceptual_owner = SharedPreprocess` and an unresolved reason; otherwise the binding is
///   derived from the interpolation's stage and marked conceptually as `SharedPreprocess`.
///
/// # Examples
///
fn interpolation_binding(
    executor: ExecutorKind,
    interpolation: &InterpolationPlan,
) -> IntermediateOpBinding {
    match interpolation.decision.realization {
        InterpolationRealization::Disabled => IntermediateOpBinding {
            owner: IntermediateOpOwner::Deferred,
            conceptual_owner: match interpolation.decision.unsupported_reason {
                Some(InterpolationUnsupportedReason::PortableFallbackNotImplemented) => {
                    IntermediateOpOwner::SharedPreprocess
                }
                _ => IntermediateOpOwner::Deferred,
            },
            accelerator: None,
            unresolved_reason: interpolation
                .decision
                .unsupported_reason
                .map(UnresolvedOperationReason::InterpolationUnsupported),
        },
        InterpolationRealization::NativeBackend => {
            let mut binding =
                binding_for_stage(interpolation.stage().unwrap_or(OpExecutionStage::Deferred));
            if executor == ExecutorKind::NvidiaSpecialized
                && interpolation.stage() == Some(OpExecutionStage::Executor)
            {
                binding.accelerator = Some(NativeAcceleratorPlan {
                    kind: NativeAcceleratorKind::NvidiaFruc,
                    fallback_owner: IntermediateOpOwner::SharedPreprocess,
                });
            }
            binding
        }
        InterpolationRealization::PortableFallback => {
            if let Some(reason) = interpolation.decision.unsupported_reason {
                IntermediateOpBinding {
                    owner: IntermediateOpOwner::Deferred,
                    conceptual_owner: IntermediateOpOwner::SharedPreprocess,
                    accelerator: None,
                    unresolved_reason: Some(UnresolvedOperationReason::InterpolationUnsupported(
                        reason,
                    )),
                }
            } else {
                let mut binding =
                    binding_for_stage(interpolation.stage().unwrap_or(OpExecutionStage::Deferred));
                binding.conceptual_owner = IntermediateOpOwner::SharedPreprocess;
                binding
            }
        }
    }
}

/// Builds an `IntermediateOpBinding` for an HDR transform plan, attaching a native accelerator

/// when the configuration and transform kind indicate executor-native NVIDIA TrueHDR support.

///

/// # Examples

///

fn hdr_transform_binding(
    executor: ExecutorKind,
    transform: &HdrTransformPlan,
) -> IntermediateOpBinding {
    let mut binding = binding_for_stage(transform.stage);

    if executor == ExecutorKind::NvidiaSpecialized
        && transform.stage == OpExecutionStage::Executor
        && matches!(
            transform.kind,
            super::plan::HdrTransformKind::NvidiaTrueHdrTonemapToHdr10
        )
    {
        binding.accelerator = Some(NativeAcceleratorPlan {
            kind: NativeAcceleratorKind::NvidiaTrueHdr,
            fallback_owner: IntermediateOpOwner::SharedPreprocess,
        });
    } else if transform.stage == OpExecutionStage::Executor {
        binding.conceptual_owner = IntermediateOpOwner::SharedPreprocess;
    }

    binding
}

/// Creates an `IntermediateOpBinding` for the specified execution stage and, when the stage
/// is `Executor`, marks the binding's conceptual owner as `SharedPreprocess`.
///
/// # Examples
///
fn shader_binding(stage: OpExecutionStage) -> IntermediateOpBinding {
    let mut binding = binding_for_stage(stage);

    if stage == OpExecutionStage::Executor {
        binding.conceptual_owner = IntermediateOpOwner::SharedPreprocess;
    }

    binding
}

/// Constructs a `SharedPreprocessOperationPlan` for an `IntermediateOperation` when that
/// operation is conceptually part of the shared preprocess stage and is a supported preprocess
/// operation.
///
/// This returns `Some` when:
/// - `operation.binding().conceptual_owner == IntermediateOpOwner::SharedPreprocess`, and
/// - the operation is one of `Resize`, `Interpolate`, `HdrTransform`, `Anime4k2xUpscale`, or
///   `Artcnn2xUpscale`.
/// The returned plan's `execution_mode` reflects the binding's concrete `owner`:
/// - `SharedPreprocess` → `PreprocessorOwned`
/// - `Executor` → `TemporaryExecutorFallback`
/// - `Deferred` → `DeferredUntilPortableRenderer`
///
/// `NormalizeInput` and `HdrMetadata` operations, or any operation whose `conceptual_owner` is
/// not `SharedPreprocess`, produce `None`.
///
/// # Examples
///
fn shared_preprocess_operation_plan(
    operation: &IntermediateOperation,
) -> Option<SharedPreprocessOperationPlan> {
    let binding = operation.binding();

    if binding.conceptual_owner != IntermediateOpOwner::SharedPreprocess {
        return None;
    }

    let execution_mode = match binding.owner {
        IntermediateOpOwner::SharedPreprocess => SharedPreprocessExecutionMode::PreprocessorOwned,
        IntermediateOpOwner::Executor => SharedPreprocessExecutionMode::TemporaryExecutorFallback,
        IntermediateOpOwner::Deferred => {
            SharedPreprocessExecutionMode::DeferredUntilPortableRenderer
        }
        IntermediateOpOwner::Normalizer | IntermediateOpOwner::Packager => return None,
    };

    let operation = match operation {
        IntermediateOperation::Resize(op) => SharedPreprocessOperation::Resize(op.clone()),
        IntermediateOperation::Interpolate(op) => {
            SharedPreprocessOperation::Interpolate(op.clone())
        }
        IntermediateOperation::HdrTransform(op) => {
            SharedPreprocessOperation::HdrTransform(op.clone())
        }
        IntermediateOperation::Anime4k2xUpscale(op) => {
            SharedPreprocessOperation::Anime4k2xUpscale(op.clone())
        }
        IntermediateOperation::Artcnn2xUpscale(op) => {
            SharedPreprocessOperation::Artcnn2xUpscale(op.clone())
        }
        IntermediateOperation::NormalizeInput(_) | IntermediateOperation::HdrMetadata(_) => {
            return None;
        }
    };

    Some(SharedPreprocessOperationPlan {
        operation,
        execution_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        IntermediateExecutionPlan, IntermediateOpOwner, IntermediateOperation,
        NativeAcceleratorKind, SharedPreprocessExecutionMode, SharedPreprocessOperation,
        UnresolvedOperationReason,
    };
    use crate::graph::{
        Anime4k2xPlan, ColorPrimariesIntent, ExecutionPlan, ExecutorKind, HdrMetadataKind,
        HdrMetadataPlan, HdrPlan, HdrRequest, HdrTransformKind, HdrTransformPlan,
        InterpolationDecision, InterpolationPlan, InterpolationUnsupportedReason, LatencyMode,
        MatrixCoefficientsIntent, OpExecutionStage, OutputBitDepth, ResizePlan,
        TransferCharacteristicIntent, VideoOp,
    };

    #[test]
    fn specialized_nvidia_plan_converts_native_ops_into_executor_bindings_with_optional_accelerators(
    ) {
        let plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![
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
                        stage: OpExecutionStage::Packager,
                    }),
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::NvidiaTrueHdrTonemapToHdr10,
                        stage: OpExecutionStage::Executor,
                    }),
                }),
            ],
        };

        let intermediate = IntermediateExecutionPlan::from_execution_plan(&plan);

        assert_eq!(intermediate.executor, ExecutorKind::NvidiaSpecialized);
        assert!(!intermediate.requires_shared_preprocess());
        assert!(!intermediate.conceptually_uses_shared_preprocess());
        assert_eq!(intermediate.operations.len(), 5);

        let resize = &intermediate.operations[1];
        assert_eq!(resize.binding().owner, IntermediateOpOwner::Executor);
        assert_eq!(
            resize.binding().accelerator.as_ref().map(|plan| plan.kind),
            Some(NativeAcceleratorKind::NvidiaNgxVsr)
        );

        let interpolation = &intermediate.operations[2];
        assert_eq!(interpolation.binding().owner, IntermediateOpOwner::Executor);
        assert_eq!(
            interpolation
                .binding()
                .accelerator
                .as_ref()
                .map(|plan| plan.kind),
            Some(NativeAcceleratorKind::NvidiaFruc)
        );

        let hdr_metadata = &intermediate.operations[3];
        assert_eq!(hdr_metadata.binding().owner, IntermediateOpOwner::Packager);
        assert_eq!(hdr_metadata.binding().accelerator, None);

        let hdr_transform = &intermediate.operations[4];
        assert_eq!(hdr_transform.binding().owner, IntermediateOpOwner::Executor);
        assert_eq!(
            hdr_transform
                .binding()
                .accelerator
                .as_ref()
                .map(|plan| plan.kind),
            Some(NativeAcceleratorKind::NvidiaTrueHdr)
        );
    }

    #[test]
    fn disabled_interpolation_becomes_deferred_and_unresolved() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::disabled(
                        InterpolationUnsupportedReason::PortableFallbackNotImplemented,
                    ),
                }),
            ],
        };

        let intermediate = plan.to_intermediate_plan();

        assert_eq!(intermediate.operations.len(), 2);
        let interpolation = &intermediate.operations[1];
        assert_eq!(interpolation.binding().owner, IntermediateOpOwner::Deferred);
        assert_eq!(
            interpolation.binding().unresolved_reason,
            Some(UnresolvedOperationReason::InterpolationUnsupported(
                InterpolationUnsupportedReason::PortableFallbackNotImplemented,
            ))
        );
        assert!(interpolation.binding().is_unresolved());
    }

    #[test]
    fn universal_executor_owned_portable_ops_are_conceptually_shared_preprocess_with_temporary_executor_fallbacks(
    ) {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
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
                    metadata: Some(HdrMetadataPlan {
                        kind: HdrMetadataKind::Hdr10Static,
                        stage: OpExecutionStage::Executor,
                    }),
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
        };

        let intermediate = plan.to_intermediate_plan();
        let preprocess_plan = intermediate.shared_preprocess_plan();

        assert!(intermediate.conceptually_uses_shared_preprocess());
        assert!(!intermediate.requires_shared_preprocess());
        assert!(!preprocess_plan.requires_stage());
        assert!(preprocess_plan.has_temporary_executor_fallbacks());
        assert!(!preprocess_plan.has_unimplemented_portable_operations());

        assert!(matches!(
            &intermediate.operations[1],
            IntermediateOperation::Resize(_)
        ));
        assert_eq!(
            intermediate.operations[1].binding().owner,
            IntermediateOpOwner::Executor
        );
        assert_eq!(intermediate.operations[1].binding().accelerator, None);
        assert_eq!(
            intermediate.operations[1].binding().conceptual_owner,
            IntermediateOpOwner::SharedPreprocess
        );
        assert_eq!(
            intermediate.operations[2].binding().owner,
            IntermediateOpOwner::Executor
        );
        assert_eq!(intermediate.operations[2].binding().accelerator, None);
        assert_eq!(
            intermediate.operations[2].binding().conceptual_owner,
            IntermediateOpOwner::Executor
        );
        assert_eq!(
            intermediate.operations[3].binding().owner,
            IntermediateOpOwner::Executor
        );
        assert_eq!(intermediate.operations[3].binding().accelerator, None);
        assert_eq!(
            intermediate.operations[3].binding().conceptual_owner,
            IntermediateOpOwner::SharedPreprocess
        );
        assert_eq!(
            intermediate.operations[4].binding().owner,
            IntermediateOpOwner::Executor
        );
        assert_eq!(
            intermediate.operations[4].binding().conceptual_owner,
            IntermediateOpOwner::SharedPreprocess
        );
        assert!(preprocess_plan.operations.iter().all(|operation| {
            operation.execution_mode == SharedPreprocessExecutionMode::TemporaryExecutorFallback
        }));
        assert!(matches!(
            &preprocess_plan.operations[0].operation,
            SharedPreprocessOperation::Resize(_)
        ));
        assert!(matches!(
            &preprocess_plan.operations[1].operation,
            SharedPreprocessOperation::HdrTransform(_)
        ));
        assert!(matches!(
            &preprocess_plan.operations[2].operation,
            SharedPreprocessOperation::Anime4k2xUpscale(_)
        ));
    }

    #[test]
    fn preprocess_stage_ops_become_first_class_shared_preprocess_work() {
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
                    metadata: None,
                    transform: Some(HdrTransformPlan {
                        kind: HdrTransformKind::TonemapToHdr10,
                        stage: OpExecutionStage::Preprocess,
                    }),
                }),
            ],
        };

        let intermediate = plan.to_intermediate_plan();
        let preprocess_plan = intermediate.shared_preprocess_plan();
        let normalization_plan = intermediate.normalization_plan();

        assert!(intermediate.requires_shared_preprocess());
        assert!(intermediate.conceptually_uses_shared_preprocess());
        assert!(preprocess_plan.requires_stage());
        assert!(!preprocess_plan.has_temporary_executor_fallbacks());
        assert_eq!(normalization_plan.operations.len(), 1);
        assert_eq!(
            normalization_plan.operations[0].binding().owner,
            IntermediateOpOwner::Normalizer
        );
        assert_eq!(
            preprocess_plan.operations[0].execution_mode,
            SharedPreprocessExecutionMode::PreprocessorOwned
        );
    }

    #[test]
    fn amd_native_resize_binding_promotes_executor_resize_out_of_shared_preprocess_fallback() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ],
        };

        let intermediate = plan
            .to_intermediate_plan()
            .with_amd_native_resize_bindings(true);
        let preprocess_plan = intermediate.shared_preprocess_plan();

        assert!(!intermediate.requires_shared_preprocess());
        assert!(!intermediate.conceptually_uses_shared_preprocess());
        assert!(!preprocess_plan.requires_stage());
        assert!(!preprocess_plan.has_temporary_executor_fallbacks());
        assert!(preprocess_plan.operations.is_empty());

        let resize = &intermediate.operations[1];
        assert_eq!(resize.binding().owner, IntermediateOpOwner::Executor);
        assert_eq!(
            resize.binding().conceptual_owner,
            IntermediateOpOwner::Executor
        );
        assert_eq!(
            resize.binding().accelerator.as_ref().map(|plan| plan.kind),
            Some(NativeAcceleratorKind::AmdVppResize)
        );
    }

    /// Verifies that a disabled portable interpolation is represented as an unimplemented deferred shared-preprocess operation.
    ///
    /// When interpolation is disabled with `PortableFallbackNotImplemented` on the `Universal` executor,
    /// the intermediate plan should mark the operation as conceptually using shared preprocess while not
    /// requiring a preprocess stage, and the derived shared preprocess plan should contain the
    /// interpolation as `DeferredUntilPortableRenderer` (unimplemented).
    ///
    /// # Examples
    ///
    #[test]
    fn disabled_portable_interpolation_stays_in_shared_preprocess_plan_as_unimplemented() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::disabled(
                        InterpolationUnsupportedReason::PortableFallbackNotImplemented,
                    ),
                }),
            ],
        };

        let intermediate = plan.to_intermediate_plan();
        let preprocess_plan = intermediate.shared_preprocess_plan();

        assert!(intermediate.conceptually_uses_shared_preprocess());
        assert!(!intermediate.requires_shared_preprocess());
        assert!(!preprocess_plan.requires_stage());
        assert!(preprocess_plan.has_unimplemented_portable_operations());
        assert_eq!(
            preprocess_plan.operations[0].execution_mode,
            SharedPreprocessExecutionMode::DeferredUntilPortableRenderer
        );
        assert!(matches!(
            &preprocess_plan.operations[0].operation,
            SharedPreprocessOperation::Interpolate(_)
        ));
    }

    /// Verifies that a portable interpolation fallback which is not implemented by the portable renderer
    /// remains deferred for execution but is still marked conceptually as shared-preprocess.
    ///
    /// This test constructs an execution plan with a portable interpolation decision that indicates
    /// the portable fallback is not implemented on the renderer, converts it to an
    /// `IntermediateExecutionPlan`, and asserts that:
    /// - the operation's concrete owner is `Deferred`,
    /// - the conceptual owner is `SharedPreprocess`,
    /// - the operation carries an `InterpolationUnsupported` unresolved reason,
    /// - the derived `SharedPreprocessPlan` reports the operation as
    ///   `DeferredUntilPortableRenderer` and as an `Interpolate` operation,
    /// - the intermediate plan conceptually uses shared preprocess but does not require a preprocess stage.
    ///
    /// # Examples
    ///
    #[test]
    fn portable_interpolation_fallback_with_renderer_gap_stays_deferred_but_not_disabled() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback_with_gap(
                        OpExecutionStage::Preprocess,
                        InterpolationUnsupportedReason::PortableFallbackNotImplemented,
                    ),
                }),
            ],
        };

        let intermediate = plan.to_intermediate_plan();
        let preprocess_plan = intermediate.shared_preprocess_plan();

        assert!(intermediate.conceptually_uses_shared_preprocess());
        assert!(!intermediate.requires_shared_preprocess());
        assert!(!preprocess_plan.requires_stage());
        assert!(preprocess_plan.has_unimplemented_portable_operations());
        assert_eq!(
            intermediate.operations[1].binding().owner,
            IntermediateOpOwner::Deferred
        );
        assert_eq!(
            intermediate.operations[1].binding().conceptual_owner,
            IntermediateOpOwner::SharedPreprocess
        );
        assert_eq!(
            intermediate.operations[1].binding().unresolved_reason,
            Some(UnresolvedOperationReason::InterpolationUnsupported(
                InterpolationUnsupportedReason::PortableFallbackNotImplemented,
            ))
        );
        assert_eq!(
            preprocess_plan.operations[0].execution_mode,
            SharedPreprocessExecutionMode::DeferredUntilPortableRenderer
        );
        assert!(matches!(
            &preprocess_plan.operations[0].operation,
            SharedPreprocessOperation::Interpolate(_)
        ));
    }
}
