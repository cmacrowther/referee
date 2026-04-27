use super::capabilities::{
    BackendCapabilities, InterpolationSupport, ResizeSupport, SelectedUpscalePath,
};
use super::plan::{
    Anime4k2xPlan, Artcnn2xPlan, ColorPrimariesIntent, ExecutionPlan, HdrMetadataKind,
    HdrMetadataPlan, HdrPlan, HdrTransformKind, HdrTransformPlan, InterpolationDecision,
    InterpolationPlan, InterpolationUnsupportedReason, MatrixCoefficientsIntent, OpExecutionStage,
    OutputBitDepth, ResizePlan, TransferCharacteristicIntent, VideoOp,
};
use super::request::{HdrRequest, InterpolationRequest, PipelineRequest, UpscaleRequest};
use super::ExecutorKind;
use crate::source::SourceContentKind;

const TARGET_INTERPOLATION_FPS: u32 = 60;

fn derive_native_fruc_target_fps(source_fps: Option<f64>) -> u32 {
    source_fps
        .filter(|fps| *fps > 0.0)
        .map(|fps| {
            ((fps * 2.0).round() as u32)
                .min(TARGET_INTERPOLATION_FPS)
                .max(1)
        })
        .unwrap_or(TARGET_INTERPOLATION_FPS)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GraphPlanner;

impl GraphPlanner {
    pub fn new() -> Self {
        Self
    }

    /// Builds an execution plan from a pipeline request and backend capabilities.
    ///
    /// The resulting `ExecutionPlan` contains the selected `executor`, the request's
    /// `latency_mode`, whether a local HLS relay is required, and an ordered list of
    /// `video_ops` beginning with `NormalizeInput` and optionally followed (in order)
    /// by an upscale operation, an interpolation operation, and an HDR operation when
    /// each is applicable to the request and capabilities.
    ///
    /// # Examples
    ///
    pub fn plan(
        &self,
        request: &PipelineRequest,
        capabilities: &BackendCapabilities,
    ) -> ExecutionPlan {
        let mut video_ops = vec![VideoOp::NormalizeInput];

        if let Some(upscale) = self.plan_upscale(request, capabilities) {
            video_ops.push(upscale);
        }

        if let Some(interpolation) = self.plan_interpolation(request, capabilities) {
            video_ops.push(VideoOp::Interpolate(interpolation));
        }

        if let Some(hdr) = self.plan_hdr(request, capabilities) {
            video_ops.push(VideoOp::Hdr(hdr));
        }

        ExecutionPlan {
            executor: capabilities.executor,
            latency_mode: request.latency_mode,
            requires_local_hls_relay: request.requires_local_hls_relay(),
            video_ops,
        }
    }

    /// Plans an upscale operation (resize or universal-shader upscale) according to the pipeline request and backend capabilities.
    ///
    /// Returns `Some(VideoOp)` describing the planned upscale operation when the request requires resizing or requests an upscale quality (including Anime4k2x treated as a quality-2 intent); returns `None` when no resizing or upscale is required.
    ///
    /// # Examples
    ///
    fn plan_upscale(
        &self,
        request: &PipelineRequest,
        capabilities: &BackendCapabilities,
    ) -> Option<VideoOp> {
        let quality = match request.upscale {
            UpscaleRequest::Off => None,
            UpscaleRequest::Quality(quality) => Some(quality),
            // Custom shader paths are Universal-only. Other executors fall
            // back to a conservative quality-2 libplacebo-style resize intent.
            UpscaleRequest::Anime4k2x | UpscaleRequest::Artcnn2x => Some(2),
        };

        let needs_resize = request
            .source_resolution
            .as_deref()
            .map(|source| source != request.output_resolution)
            .unwrap_or(false)
            || quality.is_some();

        if !needs_resize {
            return None;
        }

        match self.select_upscale_route(capabilities, quality) {
            PlannedUpscaleRoute::NativeBackend => Some(VideoOp::Resize(ResizePlan {
                target_resolution: request.output_resolution.clone(),
                quality,
                stage: OpExecutionStage::Executor,
            })),
            PlannedUpscaleRoute::SharedPreprocess => Some(VideoOp::Resize(ResizePlan {
                target_resolution: request.output_resolution.clone(),
                quality,
                stage: OpExecutionStage::Preprocess,
            })),
            PlannedUpscaleRoute::TemporaryUniversalCompatibility => self
                .plan_temporary_universal_shader_upscale(request, capabilities)
                .or_else(|| {
                    Some(VideoOp::Resize(ResizePlan {
                        target_resolution: request.output_resolution.clone(),
                        quality,
                        stage: OpExecutionStage::Executor,
                    }))
                }),
        }
    }

    /// Plans an Anime4k 2x upscale when a resize to the requested output resolution is required.
    ///
    /// Returns a `VideoOp::Anime4k2xUpscale` targeting `request.output_resolution` with execution
    /// stage `OpExecutionStage::Executor` only if the source resolution is different from the
    /// output resolution. If the source resolution is unknown, an upscale is assumed required.
    ///
    /// # Examples
    ///
    fn plan_anime4k_upscale(&self, request: &PipelineRequest) -> Option<VideoOp> {
        let needs_resize = request
            .source_resolution
            .as_deref()
            .map(|src| src != request.output_resolution)
            .unwrap_or(true);

        if !needs_resize {
            return None;
        }

        Some(VideoOp::Anime4k2xUpscale(Anime4k2xPlan {
            target_resolution: request.output_resolution.clone(),
            stage: OpExecutionStage::Executor,
        }))
    }

    /// Plans an ArtCNN 2x upscale operation when the source frame must be resized.
    ///
    /// Returns `Some(VideoOp::Artcnn2xUpscale)` targeting the request's output resolution
    /// if the source resolution is unknown or differs from the output resolution, and
    /// `None` when no resize is needed.
    ///
    /// # Examples
    ///
    fn plan_artcnn_upscale(&self, request: &PipelineRequest) -> Option<VideoOp> {
        let needs_resize = request
            .source_resolution
            .as_deref()
            .map(|src| src != request.output_resolution)
            .unwrap_or(true);

        if !needs_resize {
            return None;
        }

        Some(VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
            target_resolution: request.output_resolution.clone(),
            stage: OpExecutionStage::Executor,
        }))
    }

    /// Plans a temporary universal-shader upscale operation when the backend and request allow it.
    ///
    /// This returns a `VideoOp` that performs an upscale using a universal shader when
    /// `capabilities.executor` is `ExecutorKind::Universal` and
    /// `capabilities.upscale.selected_path` is `SelectedUpscalePath::TemporaryUniversalCompatibility`.
    /// It schedules an Anime4k2x upscale for explicit `UpscaleRequest::Anime4k2x` requests and for
    /// `UpscaleRequest::Quality(_)` when the source content is `SourceContentKind::Animated`; for
    /// `Quality(_)` with `SourceContentKind::LiveAction` or `Unknown` it schedules an ArtCNN2x upscale.
    /// Returns `None` when the request is `Off` or the backend is not eligible for temporary universal shader upscaling.
    ///
    /// # Returns
    /// `Some(VideoOp)` if an appropriate universal-shader upscale is planned, `None` otherwise.
    ///
    /// # Examples
    ///
    fn plan_temporary_universal_shader_upscale(
        &self,
        request: &PipelineRequest,
        capabilities: &BackendCapabilities,
    ) -> Option<VideoOp> {
        if capabilities.executor != ExecutorKind::Universal
            || capabilities.upscale.selected_path
                != SelectedUpscalePath::TemporaryUniversalCompatibility
        {
            return None;
        }

        if matches!(request.upscale, UpscaleRequest::Anime4k2x) {
            return self.plan_anime4k_upscale(request);
        }

        match request.upscale {
            UpscaleRequest::Off => None,
            UpscaleRequest::Quality(_) => match request.source_content_kind {
                SourceContentKind::Animated => self.plan_anime4k_upscale(request),
                SourceContentKind::LiveAction | SourceContentKind::Unknown => {
                    self.plan_artcnn_upscale(request)
                }
            },
            UpscaleRequest::Anime4k2x => self.plan_anime4k_upscale(request),
            UpscaleRequest::Artcnn2x => self.plan_artcnn_upscale(request),
        }
    }

    /// Chooses which upscale route to use for a given requested quality and backend capabilities.
    ///
    /// If the backend explicitly selected the native path and the native resize implementation
    /// supports the requested quality, the native route is chosen. Otherwise the selected path is
    /// mapped to either `SharedPreprocess` (for `NativeBackend` or `SharedPreprocess`) or
    /// `TemporaryUniversalCompatibility`.
    ///
    /// # Returns
    ///
    /// `PlannedUpscaleRoute` indicating the chosen route: `NativeBackend`, `SharedPreprocess`, or
    /// `TemporaryUniversalCompatibility`.
    ///
    /// # Examples
    ///
    fn select_upscale_route(
        &self,
        capabilities: &BackendCapabilities,
        quality: Option<u8>,
    ) -> PlannedUpscaleRoute {
        if capabilities.upscale.selected_path == SelectedUpscalePath::NativeBackend
            && native_resize_supports_request(&capabilities.resize, quality)
        {
            PlannedUpscaleRoute::NativeBackend
        } else {
            match capabilities.upscale.selected_path {
                SelectedUpscalePath::NativeBackend | SelectedUpscalePath::SharedPreprocess => {
                    PlannedUpscaleRoute::SharedPreprocess
                }
                SelectedUpscalePath::TemporaryUniversalCompatibility => {
                    PlannedUpscaleRoute::TemporaryUniversalCompatibility
                }
            }
        }
    }

    /// Plans frame-rate interpolation for the request, producing an `InterpolationPlan` when interpolation is enabled.
    ///
    /// If the request disables interpolation, returns `None`. For a request to interpolate up to 60 fps:
    /// - If the source FPS is present and already greater than or equal to 60, returns a plan with `decision` set to disabled with reason `SourceAlreadyAtOrAboveTargetFps`.
    /// - Otherwise selects a realization based on backend capabilities:
    ///   - `InterpolationSupport::To60` → native backend at executor stage, targeting double the source FPS capped at 60.
    ///   - `InterpolationSupport::Unsupported` → portable fallback at preprocess stage, targeting 60 fps.
    ///
    /// # Examples
    ///
    fn plan_interpolation(
        &self,
        request: &PipelineRequest,
        capabilities: &BackendCapabilities,
    ) -> Option<InterpolationPlan> {
        match request.interpolation {
            InterpolationRequest::Off => None,
            InterpolationRequest::To60 => {
                if request
                    .source_fps
                    .map(|fps| fps >= TARGET_INTERPOLATION_FPS as f64)
                    .unwrap_or(false)
                {
                    return Some(InterpolationPlan {
                        target_fps: TARGET_INTERPOLATION_FPS,
                        decision: InterpolationDecision::disabled(
                            InterpolationUnsupportedReason::SourceAlreadyAtOrAboveTargetFps,
                        ),
                    });
                }

                let (target_fps, decision) = match capabilities.interpolation {
                    InterpolationSupport::To60 => (
                        derive_native_fruc_target_fps(request.source_fps),
                        InterpolationDecision::native_backend(OpExecutionStage::Executor),
                    ),
                    // Route portable interpolation to the shared preprocess
                    // stage. Runtime selection can realize this with external
                    // RIFE or with FFmpeg minterpolate fallback without
                    // changing the planner contract.
                    InterpolationSupport::Unsupported => (
                        TARGET_INTERPOLATION_FPS,
                        InterpolationDecision::portable_fallback(OpExecutionStage::Preprocess),
                    ),
                };

                Some(InterpolationPlan {
                    target_fps,
                    decision,
                })
            }
        }
    }

    /// Plans HDR processing based on the pipeline's HDR intent and the backend's HDR capabilities.
    ///
    /// The returned plan specifies output bit depth, color/transfer/matrix intents, optional HDR
    /// metadata (and its execution stage), and an optional transform (and its execution stage).
    /// If the request disables HDR, no plan is produced.
    ///
    /// # Returns
    ///
    /// `Some(HdrPlan)` describing the required HDR output and where metadata/transform should execute,
    /// or `None` when `HdrRequest::Off` is requested.
    ///
    /// # Examples
    ///
    fn plan_hdr(
        &self,
        request: &PipelineRequest,
        capabilities: &BackendCapabilities,
    ) -> Option<HdrPlan> {
        match request.hdr {
            HdrRequest::Off => None,
            HdrRequest::Passthrough10Bit => Some(HdrPlan {
                request: request.hdr,
                output_bit_depth: OutputBitDepth::Bit10,
                color_primaries: ColorPrimariesIntent::PreserveSource,
                transfer: TransferCharacteristicIntent::PreserveSource,
                matrix: MatrixCoefficientsIntent::PreserveSource,
                metadata: None,
                transform: Some(HdrTransformPlan {
                    kind: HdrTransformKind::Passthrough10Bit,
                    stage: if capabilities.hdr.passthrough_10_bit {
                        OpExecutionStage::Executor
                    } else {
                        OpExecutionStage::Preprocess
                    },
                }),
            }),
            HdrRequest::TonemapToHdr10 => Some(HdrPlan {
                request: request.hdr,
                output_bit_depth: OutputBitDepth::Bit10,
                color_primaries: ColorPrimariesIntent::Bt2020,
                transfer: TransferCharacteristicIntent::Smpte2084,
                matrix: MatrixCoefficientsIntent::Bt2020Nc,
                metadata: Some(HdrMetadataPlan {
                    kind: HdrMetadataKind::Hdr10Static,
                    stage: preferred_hdr_metadata_stage(capabilities),
                }),
                transform: Some(HdrTransformPlan {
                    kind: preferred_hdr_tonemap_transform_kind(capabilities),
                    stage: if capabilities.hdr.tonemap_to_hdr10 {
                        OpExecutionStage::Executor
                    } else {
                        OpExecutionStage::Preprocess
                    },
                }),
            }),
            HdrRequest::InjectHdr10Metadata => Some(HdrPlan {
                request: request.hdr,
                output_bit_depth: OutputBitDepth::Bit10,
                color_primaries: ColorPrimariesIntent::Bt2020,
                transfer: TransferCharacteristicIntent::Smpte2084,
                matrix: MatrixCoefficientsIntent::Bt2020Nc,
                metadata: Some(HdrMetadataPlan {
                    kind: HdrMetadataKind::Hdr10Static,
                    stage: preferred_hdr_metadata_stage(capabilities),
                }),
                transform: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlannedUpscaleRoute {
    NativeBackend,
    SharedPreprocess,
    TemporaryUniversalCompatibility,
}

/// Selects the execution stage where HDR10 metadata should be injected based on backend capabilities.
///
/// Returns `OpExecutionStage::Executor` when the backend can inject HDR10 metadata; otherwise returns `OpExecutionStage::Packager`.
///
/// # Examples
///
fn preferred_hdr_metadata_stage(capabilities: &BackendCapabilities) -> OpExecutionStage {
    if capabilities.hdr.inject_hdr10_metadata {
        OpExecutionStage::Executor
    } else {
        OpExecutionStage::Packager
    }
}

fn preferred_hdr_tonemap_transform_kind(capabilities: &BackendCapabilities) -> HdrTransformKind {
    if capabilities.executor == ExecutorKind::NvidiaSpecialized && capabilities.hdr.tonemap_to_hdr10
    {
        HdrTransformKind::NvidiaTrueHdrTonemapToHdr10
    } else {
        HdrTransformKind::TonemapToHdr10
    }
}

fn native_resize_supports_request(resize: &ResizeSupport, quality: Option<u8>) -> bool {
    match resize {
        ResizeSupport::Unsupported => false,
        ResizeSupport::Basic => quality.is_none(),
        ResizeSupport::QualityRange {
            min_quality,
            max_quality,
        } => quality
            .map(|requested| (*min_quality..=*max_quality).contains(&requested))
            .unwrap_or(true),
    }
}

#[cfg(test)]
mod tests {
    use super::GraphPlanner;
    use crate::graph::{
        Anime4k2xPlan, Artcnn2xPlan, BackendCapabilities, ColorPrimariesIntent, ExecutionPlan,
        ExecutorKind, HdrMetadataKind, HdrMetadataPlan, HdrPlan, HdrRequest, HdrSupport,
        HdrTransformKind, HdrTransformPlan, InterpolationDecision, InterpolationPlan,
        InterpolationRealization, InterpolationRequest, InterpolationSupport,
        InterpolationUnsupportedReason, LatencyMode, MatrixCoefficientsIntent,
        NativeUpscaleBackend, OpExecutionStage, OutputBitDepth, PipelineRequest, ResizePlan,
        ResizeSupport, SelectedUpscalePath, TransferCharacteristicIntent,
        UpscalePlanningCapabilities, UpscaleRequest, VideoOp,
    };
    use crate::source::{SourceContentKind, SourceKind, SourceTransport};

    fn planner() -> GraphPlanner {
        GraphPlanner::new()
    }

    #[test]
    fn specialized_nvidia_plan_prefers_backend_native_execution_when_capabilities_exist() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Low,
            upscale: UpscaleRequest::Quality(3),
            interpolation: InterpolationRequest::To60,
            hdr: HdrRequest::TonemapToHdr10,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::NvidiaSpecialized,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::To60,
            hdr: HdrSupport {
                passthrough_10_bit: true,
                tonemap_to_hdr10: true,
                inject_hdr10_metadata: true,
            },
            upscale: UpscalePlanningCapabilities::native_backend(
                NativeUpscaleBackend::NvidiaNgxVsr,
            ),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan,
            ExecutionPlan {
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
                        decision: InterpolationDecision::native_backend(OpExecutionStage::Executor,),
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
            }
        );
    }

    #[test]
    fn specialized_nvidia_plan_never_emits_shader_upscale_ops_for_explicit_anime4k_requests() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Animated,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Low,
            upscale: UpscaleRequest::Anime4k2x,
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::NvidiaSpecialized,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::native_backend(
                NativeUpscaleBackend::NvidiaNgxVsr,
            ),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Executor,
                }),
            ]
        );
        assert!(
            !plan.video_ops.iter().any(|op| matches!(
                op,
                VideoOp::Anime4k2xUpscale(_) | VideoOp::Artcnn2xUpscale(_)
            )),
            "shader upscale ops are Universal-only and must not be planned for the NVIDIA specialized executor",
        );
    }

    #[test]
    fn universal_plan_prefers_shared_preprocess_interpolation_fallback_when_native_is_unavailable()
    {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(2),
            interpolation: InterpolationRequest::To60,
            hdr: HdrRequest::TonemapToHdr10,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport {
                passthrough_10_bit: true,
                tonemap_to_hdr10: true,
                inject_hdr10_metadata: true,
            },
            upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(None),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(plan.executor, ExecutorKind::Universal);
        assert!(plan.requires_local_hls_relay);
        assert_eq!(plan.video_ops[0], VideoOp::NormalizeInput);
        assert_eq!(
            plan.video_ops[1],
            VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                target_resolution: "3840x2160".to_string(),
                stage: OpExecutionStage::Executor,
            })
        );
        assert_eq!(
            plan.video_ops[2],
            VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision: InterpolationDecision::portable_fallback(OpExecutionStage::Preprocess),
            })
        );
        assert_eq!(
            plan.video_ops[3],
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
            })
        );
    }

    #[test]
    fn metadata_only_hdr_can_fall_back_to_common_packager() {
        let request = PipelineRequest {
            source_transport: SourceTransport::LocalPath,
            source_kind: SourceKind::Other,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("3840x2160".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(60.0),
            latency_mode: LatencyMode::UltraLow,
            upscale: UpscaleRequest::Off,
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::InjectHdr10Metadata,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Cpu,
            resize: ResizeSupport::Unsupported,
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::shared_preprocess(None),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Hdr(HdrPlan {
                    request: HdrRequest::InjectHdr10Metadata,
                    output_bit_depth: OutputBitDepth::Bit10,
                    color_primaries: ColorPrimariesIntent::Bt2020,
                    transfer: TransferCharacteristicIntent::Smpte2084,
                    matrix: MatrixCoefficientsIntent::Bt2020Nc,
                    metadata: Some(HdrMetadataPlan {
                        kind: HdrMetadataKind::Hdr10Static,
                        stage: OpExecutionStage::Packager,
                    }),
                    transform: None,
                }),
            ]
        );
        assert!(!plan.requires_local_hls_relay);
    }

    #[test]
    fn interpolation_request_above_target_is_disabled_with_explicit_reason() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "1920x1080".to_string(),
            source_fps: Some(60.0),
            latency_mode: LatencyMode::Low,
            upscale: UpscaleRequest::Off,
            interpolation: InterpolationRequest::To60,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::NvidiaSpecialized,
            resize: ResizeSupport::Unsupported,
            interpolation: InterpolationSupport::To60,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::shared_preprocess(None),
        };

        let plan = planner().plan(&request, &capabilities);
        let interpolation = plan.interpolation_plan().expect("interpolation decision");

        assert_eq!(
            interpolation.decision.realization,
            InterpolationRealization::Disabled
        );
        assert_eq!(
            interpolation.decision.unsupported_reason,
            Some(InterpolationUnsupportedReason::SourceAlreadyAtOrAboveTargetFps)
        );
        assert_eq!(interpolation.decision.stage, None);
    }

    #[test]
    fn native_fruc_plan_doubles_source_fps_up_to_60() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "1920x1080".to_string(),
            source_fps: Some(24.0),
            latency_mode: LatencyMode::Low,
            upscale: UpscaleRequest::Off,
            interpolation: InterpolationRequest::To60,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::NvidiaSpecialized,
            resize: ResizeSupport::Unsupported,
            interpolation: InterpolationSupport::To60,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::shared_preprocess(None),
        };

        let plan = planner().plan(&request, &capabilities);
        let interpolation = plan.interpolation_plan().expect("interpolation decision");

        assert_eq!(interpolation.target_fps, 48);
        assert_eq!(
            interpolation.decision,
            InterpolationDecision::native_backend(OpExecutionStage::Executor)
        );
    }

    #[test]
    fn unknown_source_does_not_claim_resize_without_explicit_upscale_intent() {
        let request = PipelineRequest {
            source_transport: SourceTransport::Other,
            source_kind: SourceKind::Other,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: None,
            output_resolution: "3840x2160".to_string(),
            source_fps: None,
            latency_mode: LatencyMode::Low,
            upscale: UpscaleRequest::Off,
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Cpu,
            resize: ResizeSupport::Unsupported,
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::shared_preprocess(None),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(plan.video_ops, vec![VideoOp::NormalizeInput]);
        assert!(!plan.requires_local_hls_relay);
    }

    #[test]
    fn universal_plan_switches_to_anime4k_for_detected_animation_on_upscale() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Animated,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "2560x1440".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(2),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(None),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Anime4k2xUpscale(Anime4k2xPlan {
                    target_resolution: "2560x1440".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
            ]
        );
    }

    #[test]
    fn universal_plan_switches_to_artcnn_for_detected_live_action_on_upscale() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::LiveAction,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "2560x1440".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(2),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(None),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                    target_resolution: "2560x1440".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
            ]
        );
    }

    #[test]
    fn universal_plan_defaults_unknown_content_to_artcnn_on_upscale() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "2560x1440".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(2),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(None),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Artcnn2xUpscale(Artcnn2xPlan {
                    target_resolution: "2560x1440".to_string(),
                    stage: OpExecutionStage::Executor,
                }),
            ]
        );
    }

    #[test]
    fn amd_native_upscale_plans_resize_in_executor_stage_when_vceenc_vpp_resize_available() {
        // This is the happy path for AMD native upscale:
        // VCEEncC + --vpp-resize probed → NativeBackend upscale →
        // Resize(stage: Executor) → AmdExecutor emits --output-res + --vpp-resize amf_fsr.
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(2),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        // AMD with vceenc + vpp-resize promotes to NativeBackend in the inventory
        // translation layer, so the planner sees selected_path = NativeBackend.
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::native_backend(
                NativeUpscaleBackend::AmdVceEncResize,
            ),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    // AMD native resize must be executor-owned so AmdExecutor
                    // can emit --output-res + --vpp-resize amf_fsr.
                    stage: OpExecutionStage::Executor,
                }),
            ]
        );
    }

    #[test]
    fn amd_native_upscale_falls_back_to_shared_preprocess_when_quality_exceeds_amd_range() {
        // Quality levels above the AMD FSR quality range (1–4) must fall back
        // to shared preprocess (libplacebo).  AMD has no higher-quality upscaler
        // comparable to NVIDIA NGX-VSR quality 5+.
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(5),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::native_backend(
                NativeUpscaleBackend::AmdVceEncResize,
            ),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(5),
                    // Quality 5 is out of range for AMD FSR → shared preprocess
                    // (libplacebo upscaler) handles it instead.
                    stage: OpExecutionStage::Preprocess,
                }),
            ]
        );
    }

    #[test]
    fn shared_preprocess_upscale_path_plans_resize_in_preprocess_stage() {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Balanced,
            upscale: UpscaleRequest::Quality(2),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::Universal,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities {
                selected_path: SelectedUpscalePath::SharedPreprocess,
                preferred_native_backend: None,
            },
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Preprocess,
                }),
            ]
        );
    }

    #[test]
    fn native_upscale_path_falls_back_to_shared_preprocess_when_requested_quality_is_out_of_range()
    {
        let request = PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Low,
            upscale: UpscaleRequest::Quality(5),
            interpolation: InterpolationRequest::Off,
            hdr: HdrRequest::Off,
        };
        let capabilities = BackendCapabilities {
            executor: ExecutorKind::NvidiaSpecialized,
            resize: ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            },
            interpolation: InterpolationSupport::Unsupported,
            hdr: HdrSupport::default(),
            upscale: UpscalePlanningCapabilities::native_backend(
                NativeUpscaleBackend::NvidiaNgxVsr,
            ),
        };

        let plan = planner().plan(&request, &capabilities);

        assert_eq!(
            plan.video_ops,
            vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "3840x2160".to_string(),
                    quality: Some(5),
                    stage: OpExecutionStage::Preprocess,
                }),
            ]
        );
    }
}
