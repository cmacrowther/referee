use serde::{Deserialize, Serialize};

use super::plan::ExecutorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendFamily {
    Nvidia,
    Amd,
    Universal,
    LegacyCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EncoderBinaryAvailability {
    Available,
    Missing,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FeatureAvailability {
    #[default]
    Unavailable,
    Approximate,
    Exact,
}

impl FeatureAvailability {
    /// Checks whether the feature is available.
    ///
    /// This returns `true` for `Approximate` and `Exact`, and `false` for `Unavailable`.
    ///
    /// # Examples
    ///
    pub fn is_available(self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeUpscaleBackend {
    NvidiaNgxVsr,
    AmdVceEncResize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SelectedUpscalePath {
    NativeBackend,
    SharedPreprocess,
    #[default]
    TemporaryUniversalCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpscalePlanningCapabilities {
    pub selected_path: SelectedUpscalePath,
    pub preferred_native_backend: Option<NativeUpscaleBackend>,
}

impl UpscalePlanningCapabilities {
    /// Creates an `UpscalePlanningCapabilities` that selects a native upscale backend and records the chosen backend.
    ///
    /// `preferred_native_backend` becomes the selected native backend and `selected_path` is set to `NativeBackend`.
    ///
    /// # Examples
    ///
    pub fn native_backend(preferred_native_backend: NativeUpscaleBackend) -> Self {
        Self {
            selected_path: SelectedUpscalePath::NativeBackend,
            preferred_native_backend: Some(preferred_native_backend),
        }
    }

    /// Creates an `UpscalePlanningCapabilities` configured to use the shared-preprocess path.
    ///
    /// The optional `preferred_native_backend` records a preferred native upscale backend candidate
    /// that may be used later if promotion to a native path becomes appropriate.
    ///
    /// # Examples
    ///
    pub fn shared_preprocess(preferred_native_backend: Option<NativeUpscaleBackend>) -> Self {
        Self {
            selected_path: SelectedUpscalePath::SharedPreprocess,
            preferred_native_backend,
        }
    }

    /// Constructs an UpscalePlanningCapabilities configured for temporary universal compatibility.
    ///
    /// If a `preferred_native_backend` is provided it will be recorded as a candidate while the
    /// planner remains on the temporary universal compatibility path.
    ///
    /// # Examples
    ///
    pub fn temporary_universal_compatibility(
        preferred_native_backend: Option<NativeUpscaleBackend>,
    ) -> Self {
        Self {
            selected_path: SelectedUpscalePath::TemporaryUniversalCompatibility,
            preferred_native_backend,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendFamilyCapabilities {
    pub family: BackendFamily,
    pub binary_name: String,
    pub binary_availability: EncoderBinaryAvailability,
    pub resize: FeatureAvailability,
    pub interpolation: FeatureAvailability,
    pub hdr_transform: FeatureAvailability,
    pub metadata_injection: FeatureAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapabilityInventory {
    pub selected_executor: ExecutorKind,
    pub selected_backend: BackendFamilyCapabilities,
    pub vendor_native_backend: Option<BackendFamilyCapabilities>,
    pub universal_fallback: BackendFamilyCapabilities,
}

impl BackendCapabilityInventory {
    /// Compute runtime backend capabilities from this inventory.
    ///
    /// Produces a `BackendCapabilities` value that reflects the selected executor,
    /// the selected backend family's available features (resize, interpolation,
    /// HDR-related behaviours), and an upscale planning decision derived from
    /// vendor-native candidates and the selected executor.
    ///
    /// # Examples
    ///
    pub fn to_backend_capabilities(&self) -> BackendCapabilities {
        let selected = &self.selected_backend;
        let selected_native_upscale_backend = native_upscale_backend_for_profile(selected);
        let future_native_upscale_candidate =
            self.vendor_native_backend
                .as_ref()
                .and_then(|profile| match profile.family {
                    // AMD is the one vendor-native path that can already surface
                    // separately from the planner's current `ExecutorKind`.
                    BackendFamily::Amd => native_upscale_backend_for_profile(profile),
                    _ => None,
                });

        let resize = if selected.resize.is_available() {
            ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: 4,
            }
        } else {
            ResizeSupport::Unsupported
        };

        let interpolation = if selected.interpolation.is_available() {
            InterpolationSupport::To60
        } else {
            InterpolationSupport::Unsupported
        };

        let hdr = match self.selected_executor {
            ExecutorKind::NvidiaSpecialized => HdrSupport {
                passthrough_10_bit: false,
                tonemap_to_hdr10: selected.hdr_transform.is_available(),
                inject_hdr10_metadata: selected.metadata_injection.is_available(),
            },
            ExecutorKind::Universal => HdrSupport {
                passthrough_10_bit: selected.binary_availability
                    != EncoderBinaryAvailability::Missing
                    && matches!(selected.family, BackendFamily::Universal),
                tonemap_to_hdr10: selected.hdr_transform.is_available(),
                inject_hdr10_metadata: selected.metadata_injection.is_available(),
            },
            ExecutorKind::Cpu => HdrSupport::default(),
        };

        let upscale = if let Some(native_backend) = selected_native_upscale_backend {
            UpscalePlanningCapabilities::native_backend(native_backend)
        } else if future_native_upscale_candidate == Some(NativeUpscaleBackend::AmdVceEncResize) {
            // VCEEncC with `--vpp-resize` confirmed present and available: promote
            // AMD to the native-backend upscale path so the planner stages
            // `Resize(stage: Executor)` instead of falling back to shader ops
            // (Anime4k / ArtCNN) that VCEEncC cannot execute.
            //
            // Quality levels outside AMF FSR's range still fall through to
            // shared preprocess — this is governed by `native_resize_supports_request`
            // in the planner, not here.
            //
            // The graph planner still uses `ExecutorKind::Universal` for AMD,
            // so the explicit `AmdVppResize` accelerator annotation is added
            // later at the runtime-family seam once the executor has resolved
            // to AMD/VCEEncC. The important planning contract here is that
            // native AMD upscale becomes `Resize(stage: Executor)`.
            UpscalePlanningCapabilities::native_backend(NativeUpscaleBackend::AmdVceEncResize)
        } else {
            match self.selected_executor {
                ExecutorKind::Universal => {
                    UpscalePlanningCapabilities::temporary_universal_compatibility(
                        future_native_upscale_candidate,
                    )
                }
                ExecutorKind::NvidiaSpecialized | ExecutorKind::Cpu => {
                    UpscalePlanningCapabilities::shared_preprocess(future_native_upscale_candidate)
                }
            }
        };

        BackendCapabilities {
            executor: self.selected_executor,
            resize,
            interpolation,
            hdr,
            upscale,
        }
    }
}

/// Selects a native upscale backend candidate for a backend profile when VPP resize is available.
///
/// If the profile's resize capability is available, returns the corresponding native upscale backend
/// for that backend family; otherwise returns `None`.
///
/// # Examples
///
fn native_upscale_backend_for_profile(
    profile: &BackendFamilyCapabilities,
) -> Option<NativeUpscaleBackend> {
    if !profile.resize.is_available() {
        return None;
    }

    match profile.family {
        BackendFamily::Nvidia => Some(NativeUpscaleBackend::NvidiaNgxVsr),
        BackendFamily::Amd => Some(NativeUpscaleBackend::AmdVceEncResize),
        BackendFamily::Universal | BackendFamily::LegacyCompatibility => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ResizeSupport {
    #[default]
    Unsupported,
    Basic,
    QualityRange {
        min_quality: u8,
        max_quality: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InterpolationSupport {
    #[default]
    Unsupported,
    To60,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HdrSupport {
    pub passthrough_10_bit: bool,
    pub tonemap_to_hdr10: bool,
    pub inject_hdr10_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapabilities {
    pub executor: ExecutorKind,
    pub resize: ResizeSupport,
    pub interpolation: InterpolationSupport,
    pub hdr: HdrSupport,
    pub upscale: UpscalePlanningCapabilities,
}

#[cfg(test)]
mod tests {
    use super::{
        BackendCapabilities, BackendCapabilityInventory, BackendFamily, BackendFamilyCapabilities,
        EncoderBinaryAvailability, FeatureAvailability, HdrSupport, InterpolationSupport,
        NativeUpscaleBackend, ResizeSupport, SelectedUpscalePath, UpscalePlanningCapabilities,
    };
    use crate::graph::ExecutorKind;

    #[test]
    fn universal_backend_inventory_maps_portable_processing_into_legacy_planner_capabilities() {
        let inventory = BackendCapabilityInventory {
            selected_executor: ExecutorKind::Universal,
            selected_backend: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
            vendor_native_backend: Some(BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Approximate,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
        };

        assert_eq!(
            inventory.to_backend_capabilities(),
            BackendCapabilities {
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
                // VCEEncC with --vpp-resize available: AMD is promoted to the
                // native-backend upscale path so the planner stages Resize(Executor)
                // instead of Anime4k/ArtCNN shader ops.
                upscale: UpscalePlanningCapabilities::native_backend(
                    NativeUpscaleBackend::AmdVceEncResize,
                ),
            }
        );
    }

    #[test]
    fn nvidia_backend_inventory_maps_native_exact_features_without_portable_hdr_metadata() {
        let inventory = BackendCapabilityInventory {
            selected_executor: ExecutorKind::NvidiaSpecialized,
            selected_backend: BackendFamilyCapabilities {
                family: BackendFamily::Nvidia,
                binary_name: "NVEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Exact,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Unavailable,
            },
            vendor_native_backend: None,
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
        };

        assert_eq!(
            inventory.to_backend_capabilities(),
            BackendCapabilities {
                executor: ExecutorKind::NvidiaSpecialized,
                resize: ResizeSupport::QualityRange {
                    min_quality: 1,
                    max_quality: 4,
                },
                interpolation: InterpolationSupport::To60,
                hdr: HdrSupport {
                    passthrough_10_bit: false,
                    tonemap_to_hdr10: true,
                    inject_hdr10_metadata: false,
                },
                upscale: UpscalePlanningCapabilities::native_backend(
                    NativeUpscaleBackend::NvidiaNgxVsr,
                ),
            }
        );
    }

    #[test]
    fn amd_vceenc_without_vpp_resize_stays_on_temporary_universal_compatibility_path() {
        // When VCEEncC is available but the `--vpp-resize` option was NOT found
        // during probing, AMD must not claim native upscale.  The shader ops
        // (Anime4k / ArtCNN) remain the upscale path via TemporaryUniversalCompatibility.
        let inventory = BackendCapabilityInventory {
            selected_executor: ExecutorKind::Universal,
            selected_backend: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
            vendor_native_backend: Some(BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Unavailable, // ← no vpp-resize probed
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
        };

        let translated = inventory.to_backend_capabilities();

        assert_eq!(
            translated.upscale.selected_path,
            SelectedUpscalePath::TemporaryUniversalCompatibility,
            "VCEEncC without vpp-resize must NOT promote to NativeBackend"
        );
        assert_eq!(
            translated.upscale.preferred_native_backend, None,
            "no native candidate when AMD resize is unavailable"
        );
    }

    #[test]
    fn amd_vceenc_with_vpp_resize_promotes_to_native_backend_upscale_path() {
        // When VCEEncC is present and probed `--vpp-resize` is available, AMD
        // is promoted to the native-backend upscale path.  The planner will
        // then stage Resize(Executor) so AmdExecutor emits --vpp-resize amf_fsr.
        let inventory = BackendCapabilityInventory {
            selected_executor: ExecutorKind::Universal,
            selected_backend: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
            vendor_native_backend: Some(BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Approximate,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
        };

        let translated = inventory.to_backend_capabilities();

        assert_eq!(
            translated.upscale.selected_path,
            SelectedUpscalePath::NativeBackend,
            "VCEEncC with vpp-resize must promote to NativeBackend, not TemporaryUniversalCompatibility"
        );
        assert_eq!(
            translated.upscale.preferred_native_backend,
            Some(NativeUpscaleBackend::AmdVceEncResize)
        );
    }
}
