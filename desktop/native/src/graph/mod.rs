pub mod capabilities;
pub mod intermediate;
pub mod plan;
pub mod planner;
pub mod request;

// TODO: Wire these model types into capability probing, graph planning, and
// supervisor launch once execution is split into ingest, preprocess, executor,
// and packager stages.
#[allow(unused_imports)]
pub use capabilities::{
    BackendCapabilities, BackendCapabilityInventory, BackendFamily, BackendFamilyCapabilities,
    EncoderBinaryAvailability, FeatureAvailability, HdrSupport, InterpolationSupport,
    NativeUpscaleBackend, ResizeSupport, SelectedUpscalePath, UpscalePlanningCapabilities,
};
#[allow(unused_imports)]
pub use intermediate::{
    Anime4k2xUpscaleOp, Artcnn2xUpscaleOp, HdrMetadataOp, HdrTransformOp,
    IntermediateExecutionPlan, IntermediateOpBinding, IntermediateOpOwner, IntermediateOperation,
    InterpolateOp, NativeAcceleratorKind, NativeAcceleratorPlan, NormalizeInputOp, ResizeOp,
    SharedNormalizationOperation, SharedNormalizationPlan, SharedPreprocessExecutionMode,
    SharedPreprocessOperation, SharedPreprocessOperationPlan, SharedPreprocessPlan,
    UnresolvedOperationReason,
};
#[allow(unused_imports)]
pub use plan::{
    Anime4k2xPlan, Artcnn2xPlan, ColorPrimariesIntent, ExecutionPlan, ExecutorKind,
    HdrMetadataKind, HdrMetadataPlan, HdrPlan, HdrTransformKind, HdrTransformPlan,
    InterpolationDecision, InterpolationPlan, InterpolationRealization,
    InterpolationUnsupportedReason, MatrixCoefficientsIntent, OpExecutionStage, OutputBitDepth,
    ResizePlan, TransferCharacteristicIntent, VideoOp,
};
#[allow(unused_imports)]
pub use planner::GraphPlanner;
#[allow(unused_imports)]
pub use request::{HdrRequest, InterpolationRequest, LatencyMode, PipelineRequest, UpscaleRequest};
