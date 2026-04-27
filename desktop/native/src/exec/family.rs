use crate::graph::{
    BackendFamily, BackendFamilyCapabilities, EncoderBinaryAvailability, ExecutorKind,
    FeatureAvailability,
};
use crate::pipeline::EncoderCapabilities;

use super::{AmdExecutorCapabilities, RuntimePlatform};

/// The concrete executor implementation family selected for a given pipeline run.
///
/// This is a *runtime* concept, distinct from [`ExecutorKind`] which is a *graph-planning* concept.
/// The planner emits `ExecutorKind` to describe the desired execution strategy; this module
/// maps that to a concrete `ExecutorFamily` once the actual GPU vendor and available binaries
/// are known. Notably, `ExecutorKind::Universal` maps to either `ExecutorFamily::Amd` or
/// `ExecutorFamily::Universal` depending on the detected vendor — the planner does not
/// distinguish between the two at plan time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorFamily {
    Nvidia,
    Amd,
    Universal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeExecutorFamilyDecision {
    pub family: ExecutorFamily,
    pub amd_capabilities: Option<AmdExecutorCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeExecutorTarget {
    Family(RuntimeExecutorFamilyDecision),
    LegacyCompatibility,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeExecutorFamilyContext<'a> {
    pub executor_kind: ExecutorKind,
    pub platform: RuntimePlatform,
    pub gpu_vendor: &'a str,
    pub encoder_backend: &'a str,
    pub encoder_capabilities: &'a EncoderCapabilities,
}

/// Selects the runtime executor target (a selected family decision or legacy compatibility)
/// based on the provided execution context.
///
/// The selection rules are:
/// - `ExecutorKind::NvidiaSpecialized` selects `ExecutorFamily::Nvidia`.
/// - `ExecutorKind::Cpu` selects `RuntimeExecutorTarget::LegacyCompatibility`.
/// - `ExecutorKind::Universal` selects `ExecutorFamily::Amd` when `gpu_vendor` equals
///   `"amd"` (case-insensitive), producing AMD capabilities derived from `encoder_backend`
///   and `encoder_capabilities`; otherwise it selects `ExecutorFamily::Universal`.
///
/// # Returns
///
/// A `RuntimeExecutorTarget` representing either a family decision (with optional AMD
/// capabilities) or `LegacyCompatibility`.
///
/// # Examples
///
pub(crate) fn select_runtime_executor_target(
    context: RuntimeExecutorFamilyContext<'_>,
) -> RuntimeExecutorTarget {
    let _platform = context.platform;

    match context.executor_kind {
        ExecutorKind::NvidiaSpecialized => {
            RuntimeExecutorTarget::Family(RuntimeExecutorFamilyDecision {
                family: ExecutorFamily::Nvidia,
                amd_capabilities: None,
            })
        }
        ExecutorKind::Universal => {
            if context.gpu_vendor.trim().eq_ignore_ascii_case("amd") {
                let amd_profile = BackendFamilyCapabilities {
                    family: BackendFamily::Amd,
                    binary_name: "VCEEncC".to_string(),
                    binary_availability: if context
                        .encoder_backend
                        .trim()
                        .eq_ignore_ascii_case("vceenc")
                    {
                        EncoderBinaryAvailability::Available
                    } else {
                        EncoderBinaryAvailability::Unknown
                    },
                    resize: if context.encoder_capabilities.has_vpp_resize {
                        FeatureAvailability::Approximate
                    } else {
                        FeatureAvailability::Unavailable
                    },
                    interpolation: FeatureAvailability::Unavailable,
                    hdr_transform: FeatureAvailability::Unavailable,
                    metadata_injection: FeatureAvailability::Unavailable,
                };
                RuntimeExecutorTarget::Family(RuntimeExecutorFamilyDecision {
                    family: ExecutorFamily::Amd,
                    amd_capabilities: Some(
                        AmdExecutorCapabilities::from_backend_capability_profile(&amd_profile),
                    ),
                })
            } else {
                RuntimeExecutorTarget::Family(RuntimeExecutorFamilyDecision {
                    family: ExecutorFamily::Universal,
                    amd_capabilities: None,
                })
            }
        }
        ExecutorKind::Cpu => RuntimeExecutorTarget::LegacyCompatibility,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        select_runtime_executor_target, ExecutorFamily, RuntimeExecutorFamilyContext,
        RuntimeExecutorTarget,
    };
    use crate::exec::RuntimePlatform;
    use crate::graph::ExecutorKind;
    use crate::pipeline::EncoderCapabilities;

    /// Construct a canonical EncoderCapabilities value used in tests: VPP resize enabled, all other features disabled.
    ///
    /// # Returns
    ///
    /// `EncoderCapabilities` with `has_vpp_resize = true` and `has_fruc`, `has_truehdr`, `has_rife` all set to `false`.
    ///
    /// # Examples
    ///
    fn encoder_capabilities() -> EncoderCapabilities {
        EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: false,
            has_truehdr: false,
            has_rife: false,
        }
    }

    #[test]
    fn universal_amd_runtime_routes_to_amd_family_with_honest_vceenc_capabilities() {
        let target = select_runtime_executor_target(RuntimeExecutorFamilyContext {
            executor_kind: ExecutorKind::Universal,
            platform: RuntimePlatform::Windows,
            gpu_vendor: "amd",
            encoder_backend: "vceenc",
            encoder_capabilities: &encoder_capabilities(),
        });

        match target {
            RuntimeExecutorTarget::Family(decision) => {
                assert_eq!(decision.family, ExecutorFamily::Amd);
                let amd_capabilities = decision.amd_capabilities.expect("amd capabilities");
                assert!(amd_capabilities.vceenc_available);
                assert!(amd_capabilities.supports_native_upscale());
            }
            RuntimeExecutorTarget::LegacyCompatibility => {
                panic!("expected amd runtime family selection")
            }
        }
    }

    #[test]
    fn universal_nvidia_runtime_stays_on_universal_family_when_graph_plan_is_universal() {
        let target = select_runtime_executor_target(RuntimeExecutorFamilyContext {
            executor_kind: ExecutorKind::Universal,
            platform: RuntimePlatform::Linux,
            gpu_vendor: "nvidia",
            encoder_backend: "ffmpeg",
            encoder_capabilities: &encoder_capabilities(),
        });

        assert_eq!(
            target,
            RuntimeExecutorTarget::Family(super::RuntimeExecutorFamilyDecision {
                family: ExecutorFamily::Universal,
                amd_capabilities: None,
            })
        );
    }

    #[test]
    fn cpu_runtime_stays_on_legacy_compatibility_target() {
        let target = select_runtime_executor_target(RuntimeExecutorFamilyContext {
            executor_kind: ExecutorKind::Cpu,
            platform: RuntimePlatform::Windows,
            gpu_vendor: "unknown",
            encoder_backend: "ffmpeg",
            encoder_capabilities: &encoder_capabilities(),
        });

        assert_eq!(target, RuntimeExecutorTarget::LegacyCompatibility);
    }
}
