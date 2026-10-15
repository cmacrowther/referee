//! Encoder capability probing and executor-kind selection, which run before
//! graph planning.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::exec::{parse_nvenc_capabilities, RuntimePlatform};
use crate::graph::{
    BackendCapabilities, BackendCapabilityInventory, BackendFamily, BackendFamilyCapabilities,
    EncoderBinaryAvailability, ExecutorKind, HdrRequest, PipelineRequest, UpscaleRequest,
};

use super::MAX_VSR_QUALITY;

/// Encoder-native features probed from the encoder's `--help` output, plus
/// whether a `rife-worker` is available.
#[derive(Debug, Clone, Serialize)]
pub struct EncoderCapabilities {
    pub has_vpp_resize: bool,
    pub has_fruc: bool,
    pub has_truehdr: bool,
    /// True when a compiled `rife-worker` binary + a model directory are
    /// available for frame interpolation preprocessing.
    pub has_rife: bool,
    /// Native frame doubling: NVEncC NVIDIA Framegen (`--vpp-nvvfx-framegen`)
    /// on an Ada or newer GPU, or VCEEncC AMF FRC (`--vpp-frc`) on Windows.
    pub has_native_double: bool,
    /// VCEEncC SDR-to-HDR10 through libplacebo (`--vpp-libplacebo-tonemapping`).
    pub has_hdr_tonemap: bool,
}

impl EncoderCapabilities {
    /// Sets whether an external RIFE binary is available for preprocessing.
    pub fn with_rife(mut self, has_rife: bool) -> Self {
        self.has_rife = has_rife;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ExecutorPreference {
    #[default]
    Auto,
    #[serde(alias = "nvidia", alias = "nvidiaSpecialized")]
    NvidiaAi,
    #[serde(alias = "amd")]
    AmdAi,
    Universal,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExecutorSelectionContext<'a> {
    pub platform: RuntimePlatform,
    pub gpu_vendor: &'a str,
    pub request: &'a PipelineRequest,
    pub encoder_capabilities: &'a EncoderCapabilities,
    pub executor_preference: ExecutorPreference,
}

/// Chooses the executor family before graph planning.
///
/// - The specialized NVEncC path is selected for Windows + NVIDIA when a
///   requested operation can use a probed specialized capability.
/// - The specialized VCEEncC path is selected whenever VCEEncC is available on
///   an AMD GPU (Windows or Linux), as the AMD runtime always preferred it.
/// - Universal is the default for portable FFmpeg processing and the
///   hardware-specific FFmpeg encode backends.
pub(crate) fn select_executor_kind_for_request(
    context: ExecutorSelectionContext<'_>,
) -> ExecutorKind {
    match context.executor_preference {
        ExecutorPreference::Universal => return ExecutorKind::Universal,
        ExecutorPreference::NvidiaAi => {
            return if nvidia_ai_available(
                context.platform,
                context.gpu_vendor,
                context.encoder_capabilities,
            ) {
                ExecutorKind::NvidiaSpecialized
            } else {
                ExecutorKind::Universal
            };
        }
        ExecutorPreference::AmdAi => {
            return if amd_ai_available(
                context.platform,
                context.gpu_vendor,
                context.encoder_capabilities,
            ) {
                ExecutorKind::AmdSpecialized
            } else {
                ExecutorKind::Universal
            };
        }
        ExecutorPreference::Auto => {}
    }

    if context.platform == RuntimePlatform::Windows
        && is_vendor(context.gpu_vendor, "nvidia")
        && specialized_nvidia_benefits_request(context.request, context.encoder_capabilities)
    {
        ExecutorKind::NvidiaSpecialized
    } else if amd_ai_available(
        context.platform,
        context.gpu_vendor,
        context.encoder_capabilities,
    ) {
        ExecutorKind::AmdSpecialized
    } else {
        ExecutorKind::Universal
    }
}

fn is_vendor(gpu_vendor: &str, vendor: &str) -> bool {
    gpu_vendor.trim().eq_ignore_ascii_case(vendor)
}

/// Whether the NVIDIA specialized executor can run at all: Windows, an NVIDIA
/// GPU, and at least one NGX/FRUC/TrueHDR capability.
pub(crate) fn nvidia_ai_available(
    platform: RuntimePlatform,
    gpu_vendor: &str,
    capabilities: &EncoderCapabilities,
) -> bool {
    platform == RuntimePlatform::Windows
        && is_vendor(gpu_vendor, "nvidia")
        && (capabilities.has_vpp_resize || capabilities.has_fruc || capabilities.has_truehdr)
}

/// Whether the AMD specialized executor can run: an AMD GPU with VCEEncC
/// (whose `--vpp-resize` marks it as present).
pub(crate) fn amd_ai_available(
    _platform: RuntimePlatform,
    gpu_vendor: &str,
    capabilities: &EncoderCapabilities,
) -> bool {
    is_vendor(gpu_vendor, "amd") && capabilities.has_vpp_resize
}

/// Whether the request asks for an upscale, frame generation, or HDR
/// transform that the specialized NVIDIA executor can render natively.
fn specialized_nvidia_benefits_request(
    request: &PipelineRequest,
    capabilities: &EncoderCapabilities,
) -> bool {
    let upscale = capabilities.has_vpp_resize
        && matches!(
            request.upscale,
            UpscaleRequest::Quality(quality) if (1..=MAX_VSR_QUALITY).contains(&quality)
        );
    // Any frame generation keeps NVEncC as the executor, including RIFE (which
    // runs in its own preprocess stage), so switching engines never changes
    // which executor and engine options apply.
    let framegen =
        capabilities.has_fruc && request.interpolation.raises_frame_rate(request.source_fps);
    let hdr = capabilities.has_truehdr && request.hdr == HdrRequest::TonemapToHdr10;
    upscale || framegen || hdr
}

impl EncoderCapabilities {
    /// Derives the compact capability flags from a probed backend profile.
    /// Only encoder-native features are inferred; `has_rife` stays false
    /// (set it with [`EncoderCapabilities::with_rife`]).
    pub(crate) fn from_backend_capability_profile(profile: &BackendFamilyCapabilities) -> Self {
        Self {
            has_vpp_resize: matches!(profile.family, BackendFamily::Nvidia | BackendFamily::Amd)
                && profile.resize.is_available(),
            has_fruc: matches!(profile.family, BackendFamily::Nvidia)
                && profile.interpolation.is_available(),
            has_truehdr: matches!(profile.family, BackendFamily::Nvidia)
                && profile.hdr_transform.is_available(),
            has_rife: false,
            has_native_double: matches!(profile.family, BackendFamily::Nvidia | BackendFamily::Amd)
                && profile.frame_generation.is_available(),
            has_hdr_tonemap: matches!(profile.family, BackendFamily::Amd)
                && profile.hdr_transform.is_available(),
        }
    }

    /// Expands the capability flags back into the vendor-native backend
    /// profile for `gpu_vendor`. The binary counts as available only when
    /// `encoder_backend` is that vendor's CLI. Returns `None` for vendors
    /// other than NVIDIA and AMD.
    pub(crate) fn vendor_native_backend_capabilities(
        &self,
        gpu_vendor: &str,
        encoder_backend: &str,
    ) -> Option<BackendFamilyCapabilities> {
        let availability = |native_backend: &str| {
            if encoder_backend.trim().eq_ignore_ascii_case(native_backend) {
                EncoderBinaryAvailability::Available
            } else {
                EncoderBinaryAvailability::Unknown
            }
        };

        if is_vendor(gpu_vendor, "nvidia") {
            Some(BackendFamilyCapabilities::nvidia(
                availability("nvenc"),
                self.has_vpp_resize,
                self.has_fruc,
                self.has_native_double,
                self.has_truehdr,
            ))
        } else if is_vendor(gpu_vendor, "amd") {
            Some(BackendFamilyCapabilities::amd(
                availability("vceenc"),
                self.has_vpp_resize,
                self.has_native_double,
                self.has_hdr_tonemap,
            ))
        } else {
            None
        }
    }

    /// Selects an executor kind for the request and assembles the selected
    /// backend profile together with the vendor-native and universal
    /// alternatives. The vendor-native candidate is dropped when it is the
    /// same family as the selected backend.
    pub(crate) fn to_backend_capability_inventory_for_request(
        &self,
        platform: RuntimePlatform,
        gpu_vendor: &str,
        encoder_backend: &str,
        request: &PipelineRequest,
        executor_preference: ExecutorPreference,
    ) -> BackendCapabilityInventory {
        let executor = select_executor_kind_for_request(ExecutorSelectionContext {
            platform,
            gpu_vendor,
            request,
            encoder_capabilities: self,
            executor_preference,
        });
        let universal_fallback =
            BackendFamilyCapabilities::universal(EncoderBinaryAvailability::Available);
        let vendor_native_backend =
            self.vendor_native_backend_capabilities(gpu_vendor, encoder_backend);
        let legacy = || {
            BackendFamilyCapabilities::legacy(encoder_backend, EncoderBinaryAvailability::Unknown)
        };
        let selected_backend = match executor {
            ExecutorKind::NvidiaSpecialized | ExecutorKind::AmdSpecialized => {
                vendor_native_backend.clone().unwrap_or_else(legacy)
            }
            ExecutorKind::Universal => universal_fallback.clone(),
            ExecutorKind::Cpu => legacy(),
        };
        let vendor_native_backend =
            vendor_native_backend.filter(|profile| profile.family != selected_backend.family);

        BackendCapabilityInventory {
            selected_executor: executor,
            selected_backend,
            vendor_native_backend,
            universal_fallback,
        }
    }

    /// The planner's view of the backend for this request and runtime.
    pub(crate) fn to_backend_capabilities_for_request(
        &self,
        platform: RuntimePlatform,
        gpu_vendor: &str,
        encoder_backend: &str,
        request: &PipelineRequest,
        executor_preference: ExecutorPreference,
    ) -> BackendCapabilities {
        self.to_backend_capability_inventory_for_request(
            platform,
            gpu_vendor,
            encoder_backend,
            request,
            executor_preference,
        )
        .to_backend_capabilities()
    }
}

/// Maps an encoder's `--help` text to a backend capability profile.
///
/// - `"nvenc"`: NVIDIA family, features from [`parse_nvenc_capabilities`].
/// - `"vceenc"`: AMD family; `--vpp-resize` in the help means approximate resize.
/// - `"ffmpeg"`: Universal family with resize, HDR transform, and metadata injection.
/// - anything else: legacy compatibility with no features.
pub(crate) fn parse_encoder_backend_capabilities(
    help_text: &str,
    backend: &str,
) -> BackendFamilyCapabilities {
    let available = EncoderBinaryAvailability::Available;
    match backend.trim().to_ascii_lowercase().as_str() {
        "nvenc" => {
            let parsed = parse_nvenc_capabilities(help_text);
            BackendFamilyCapabilities::nvidia(
                available,
                parsed.has_vpp_resize,
                parsed.has_fruc,
                parsed.has_native_double,
                parsed.has_truehdr,
            )
        }
        "vceenc" => BackendFamilyCapabilities::amd(
            available,
            help_text.contains("--vpp-resize"),
            help_text.contains("--vpp-frc"),
            help_text.contains("--vpp-libplacebo-tonemapping"),
        ),
        "ffmpeg" => BackendFamilyCapabilities::universal(available),
        other => BackendFamilyCapabilities::legacy(other, available),
    }
}

/// Probes the encoder's `--help` output (cached per binary) into a backend
/// capability profile. If the encoder cannot run, the profile reports the
/// binary as missing with no encoder-native features.
fn detect_encoder_backend_capabilities(
    encoder_path: &Path,
    backend: &str,
) -> BackendFamilyCapabilities {
    match crate::process::cached_probe_output(encoder_path, &["--help"]) {
        Ok(out) => {
            let mut help_text = String::from_utf8_lossy(&out.stdout).to_string();
            help_text.push_str(&String::from_utf8_lossy(&out.stderr));
            parse_encoder_backend_capabilities(&help_text, backend)
        }
        Err(_) => BackendFamilyCapabilities {
            binary_availability: EncoderBinaryAvailability::Missing,
            ..parse_encoder_backend_capabilities("", backend)
        },
    }
}

/// Probes an encoder binary into the planner-facing [`EncoderCapabilities`].
/// `backend` is the encoder family identifier (`"nvenc"`, `"vceenc"`, or
/// `"ffmpeg"`).
pub fn detect_encoder_capabilities(encoder_path: &Path, backend: &str) -> EncoderCapabilities {
    let capabilities = EncoderCapabilities::from_backend_capability_profile(
        &detect_encoder_backend_capabilities(encoder_path, backend),
    );
    // Native doubling has requirements the help text does not report: NVIDIA
    // Framegen needs an Ada or newer GPU, and VCEEncC runs AMF FRC on its DX11
    // engine, so Windows only.
    let native_double_supported = match backend.trim().to_ascii_lowercase().as_str() {
        "nvenc" => crate::gpu::nvidia_gpu_supports_frame_generation(),
        "vceenc" => cfg!(windows),
        _ => false,
    };
    EncoderCapabilities {
        has_native_double: capabilities.has_native_double && native_double_supported,
        ..capabilities
    }
}
