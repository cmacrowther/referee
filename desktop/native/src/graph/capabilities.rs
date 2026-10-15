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

    /// `when_available` if `available`, otherwise `Unavailable`.
    pub fn when(available: bool, when_available: Self) -> Self {
        if available {
            when_available
        } else {
            Self::Unavailable
        }
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
    /// Whole-number frame multiplication (NVIDIA frame generation).
    #[serde(default)]
    pub frame_generation: FeatureAvailability,
    pub hdr_transform: FeatureAvailability,
    pub metadata_injection: FeatureAvailability,
}

impl BackendFamilyCapabilities {
    /// NVEncC. NGX VSR resize, FRUC interpolation, NVIDIA frame generation,
    /// and TrueHDR are exact when present; HDR metadata injection is not
    /// offered.
    pub fn nvidia(
        binary_availability: EncoderBinaryAvailability,
        has_resize: bool,
        has_fruc: bool,
        has_framegen: bool,
        has_truehdr: bool,
    ) -> Self {
        Self {
            family: BackendFamily::Nvidia,
            binary_name: "NVEncC".to_string(),
            binary_availability,
            resize: FeatureAvailability::when(has_resize, FeatureAvailability::Exact),
            interpolation: FeatureAvailability::when(has_fruc, FeatureAvailability::Exact),
            frame_generation: FeatureAvailability::when(has_framegen, FeatureAvailability::Exact),
            hdr_transform: FeatureAvailability::when(has_truehdr, FeatureAvailability::Exact),
            metadata_injection: FeatureAvailability::Unavailable,
        }
    }

    /// VCEEncC. AMF FSR resize (approximate: no AI model), AMF FRC frame
    /// doubling, libplacebo SDR-to-HDR10, and HDR10 metadata injection when
    /// present. No any-target interpolation.
    pub fn amd(
        binary_availability: EncoderBinaryAvailability,
        has_resize: bool,
        has_frc: bool,
        has_hdr_tonemap: bool,
    ) -> Self {
        Self {
            family: BackendFamily::Amd,
            binary_name: "VCEEncC".to_string(),
            binary_availability,
            resize: FeatureAvailability::when(has_resize, FeatureAvailability::Approximate),
            interpolation: FeatureAvailability::Unavailable,
            frame_generation: FeatureAvailability::when(has_frc, FeatureAvailability::Exact),
            hdr_transform: FeatureAvailability::when(has_hdr_tonemap, FeatureAvailability::Exact),
            metadata_injection: FeatureAvailability::when(
                has_hdr_tonemap,
                FeatureAvailability::Exact,
            ),
        }
    }

    /// FFmpeg + libplacebo: exact resize, HDR transform, and metadata
    /// injection, but no interpolation.
    pub fn universal(binary_availability: EncoderBinaryAvailability) -> Self {
        Self {
            family: BackendFamily::Universal,
            binary_name: "ffmpeg".to_string(),
            binary_availability,
            resize: FeatureAvailability::Exact,
            interpolation: FeatureAvailability::Unavailable,
            frame_generation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Exact,
            metadata_injection: FeatureAvailability::Exact,
        }
    }

    /// An unrecognized encoder: no features.
    pub fn legacy(binary_name: &str, binary_availability: EncoderBinaryAvailability) -> Self {
        Self {
            family: BackendFamily::LegacyCompatibility,
            binary_name: binary_name.to_string(),
            binary_availability,
            resize: FeatureAvailability::Unavailable,
            interpolation: FeatureAvailability::Unavailable,
            frame_generation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        }
    }
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
        // An AMD-native upscaler the selected executor is not using (the
        // Universal preference on an AMD GPU), reported for diagnostics only.
        let future_native_upscale_candidate =
            self.vendor_native_backend
                .as_ref()
                .and_then(|profile| match profile.family {
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

        let interpolation = match (
            selected.interpolation.is_available(),
            selected.frame_generation.is_available(),
        ) {
            (true, true) => InterpolationSupport::NativeWithFramegen,
            (true, false) => InterpolationSupport::Native,
            (false, true) => InterpolationSupport::FramegenOnly,
            (false, false) => InterpolationSupport::Unsupported,
        };

        let hdr = match self.selected_executor {
            ExecutorKind::NvidiaSpecialized | ExecutorKind::AmdSpecialized => HdrSupport {
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
        } else {
            match self.selected_executor {
                ExecutorKind::Universal => {
                    UpscalePlanningCapabilities::temporary_universal_compatibility(
                        future_native_upscale_candidate,
                    )
                }
                ExecutorKind::NvidiaSpecialized
                | ExecutorKind::AmdSpecialized
                | ExecutorKind::Cpu => {
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
    /// The selected backend interpolates to any target frame rate (NVEncC FRUC).
    Native,
    /// FRUC, plus NVIDIA frame generation for whole-number multiples.
    NativeWithFramegen,
    /// Native frame doubling only (AMD Fluid Motion); other targets need a
    /// portable or RIFE stage.
    FramegenOnly,
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
                frame_generation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
            vendor_native_backend: Some(BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Approximate,
                interpolation: FeatureAvailability::Unavailable,
                frame_generation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                frame_generation: FeatureAvailability::Unavailable,
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
                // The Universal executor renders upscales itself; the unused
                // VCEEncC resize is only reported as a candidate.
                upscale: UpscalePlanningCapabilities::temporary_universal_compatibility(Some(
                    NativeUpscaleBackend::AmdVceEncResize,
                )),
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
                frame_generation: FeatureAvailability::Unavailable,
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
                frame_generation: FeatureAvailability::Unavailable,
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
                interpolation: InterpolationSupport::Native,
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
                frame_generation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
            vendor_native_backend: Some(BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Unavailable, // ← no vpp-resize probed
                interpolation: FeatureAvailability::Unavailable,
                frame_generation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                frame_generation: FeatureAvailability::Unavailable,
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
    fn amd_specialized_inventory_maps_fsr_frc_and_hdr_to_native_planner_capabilities() {
        let inventory = BackendCapabilityInventory {
            selected_executor: ExecutorKind::AmdSpecialized,
            selected_backend: BackendFamilyCapabilities::amd(
                EncoderBinaryAvailability::Available,
                true,
                true,
                true,
            ),
            vendor_native_backend: None,
            universal_fallback: BackendFamilyCapabilities {
                family: BackendFamily::Universal,
                binary_name: "ffmpeg".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: FeatureAvailability::Exact,
                interpolation: FeatureAvailability::Unavailable,
                frame_generation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Exact,
                metadata_injection: FeatureAvailability::Exact,
            },
        };

        let translated = inventory.to_backend_capabilities();

        assert_eq!(translated.executor, ExecutorKind::AmdSpecialized);
        assert_eq!(
            translated.upscale.selected_path,
            SelectedUpscalePath::NativeBackend
        );
        assert_eq!(
            translated.upscale.preferred_native_backend,
            Some(NativeUpscaleBackend::AmdVceEncResize)
        );
        // AMF FRC doubles only; fixed targets stay portable.
        assert_eq!(translated.interpolation, InterpolationSupport::FramegenOnly);
        assert_eq!(
            translated.hdr,
            HdrSupport {
                passthrough_10_bit: false,
                tonemap_to_hdr10: true,
                inject_hdr10_metadata: true,
            }
        );
    }
}
