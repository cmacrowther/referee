use crate::deps::detect_ffmpeg_filter;
use crate::exec::{
    parse_nvenc_capabilities, select_runtime_executor_target, select_universal_encode_backend,
    AmdExecutor, AmdExecutorContext, ExecutorFamily, FfmpegHlsPackager, FfmpegHlsPackagerContext,
    NvidiaSpecializedExecutor, NvidiaSpecializedExecutorContext, RuntimeExecutorFamilyContext,
    RuntimeExecutorTarget, RuntimePlatform, UniversalBackendSelectionContext, UniversalExecutor,
    UniversalExecutorContext,
};
use crate::graph::{
    BackendCapabilities, BackendCapabilityInventory, BackendFamily, BackendFamilyCapabilities,
    EncoderBinaryAvailability, ExecutionPlan, ExecutorKind, FeatureAvailability, HdrRequest,
    IntermediateExecutionPlan, InterpolationRequest, PipelineRequest, UpscaleRequest,
};
use crate::normalize::FfmpegNormalizer;
use crate::preprocess::{FfmpegPreprocessor, StreamingRifePreprocessor};
use crate::runtime::{
    BackendExecutor, FrameTransport, LegacySingleProcessExecutor,
    LegacySingleProcessExecutorContext, LegacyStartupMode, Normalizer, Packager, PipelineStageId,
    PipelineSupervisor, PipelineSupervisorStopReason, PipelineSupervisorTimeouts, Preprocessor,
    StageRuntimeContext, TransportConfig,
};
use crate::settings::RelayPeerMetadata;
use crate::source::{SourceContentKind, SourceDescriptor};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::{debug, error, info, warn};

const MAX_PIPELINE_START_ATTEMPTS: u32 = 2;
/// CAS (FidelityFX Contrast Adaptive Sharpening) strength applied at the end of every
/// executor's filter chain. Range [0.0, 1.0]; the sweet spot is roughly 0.4–0.7.
const CAS_DEFAULT_STRENGTH: f32 = 0.5;
#[cfg(test)]
const FFMPEG_PROGRAM: &str = "ffmpeg";
/// How long to wait without a heartbeat before killing a session that has already checked in.
const HEARTBEAT_TIMEOUT_MS: u64 = 15_000;
/// How long to wait for the very first heartbeat before treating the session as orphaned.
/// This needs to be long enough to cover GPU startup + packager playlist
/// appearance + client round-trip.
const ORPHAN_SESSION_TIMEOUT_MS: u64 = 300_000;
const PIPELINE_CLEANUP_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingProfile {
    pub bitrate: u32,
    pub max_bitrate: u32,
    pub preset: String,
    pub lookahead: u32,
    pub bframes: u32,
    pub hls_time: u32,
}

pub fn get_encoding_profile(resolution: &str) -> EncodingProfile {
    match resolution {
        "2560x1440" => EncodingProfile {
            bitrate: 35000,
            max_bitrate: 52500,
            preset: "p4".to_string(),
            lookahead: 12,
            bframes: 3,
            hls_time: 1,
        },
        "3840x2160" => EncodingProfile {
            bitrate: 50000,
            max_bitrate: 75000,
            preset: "p4".to_string(),
            lookahead: 8,
            bframes: 3,
            hls_time: 1,
        },
        _ => EncodingProfile {
            bitrate: 25000,
            max_bitrate: 37500,
            preset: "p4".to_string(),
            lookahead: 8,
            bframes: 3,
            hls_time: 1,
        },
    }
}

fn round_frame_rate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Reports whether the execution plan includes an interpolation step that is executable.
///
/// Checks the plan's interpolation sub-plan and returns `true` when that sub-plan exists and is executable, `false` otherwise.
///
/// # Returns
/// `true` if the execution plan's interpolation plan is present and executable, `false` otherwise.
pub fn is_interpolation_enabled(execution_plan: &ExecutionPlan) -> bool {
    execution_plan
        .interpolation_plan()
        .map(|interpolation| interpolation.is_executable())
        .unwrap_or(false)
}

/// Determine the planned target frame rate for encoding based on the source FPS and the execution plan's interpolation.
///
/// If `source_fps` is `None` or less than or equal to 0, returns `None`. If the execution plan contains an executable
/// interpolation plan, returns the interpolation plan's target FPS. If no executable interpolation is present, returns
/// the source FPS. The returned value is rounded to two decimal places.
///
/// # Examples
///
pub fn planned_target_frame_rate(
    source_fps: Option<f64>,
    execution_plan: &ExecutionPlan,
) -> Option<f64> {
    let interpolation = execution_plan.interpolation_plan();

    source_fps.and_then(|fps| {
        if fps > 0.0 {
            let target = match interpolation {
                Some(interpolation) if interpolation.is_executable() => {
                    interpolation.target_fps as f64
                }
                _ => fps,
            };

            Some(round_frame_rate(target))
        } else {
            None
        }
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EncoderCapabilities {
    pub has_vpp_resize: bool,
    pub has_fruc: bool,
    pub has_truehdr: bool,
    /// True when a compiled `rife-worker` binary + a model directory are
    /// available for frame interpolation preprocessing.
    pub has_rife: bool,
}

impl EncoderCapabilities {
    /// Sets whether an external RIFE binary is available for preprocessing.
    pub fn with_rife(mut self, has_rife: bool) -> Self {
        self.has_rife = has_rife;
        self
    }
}

/// Parameters for the RIFE worker preprocessor path.
#[derive(Debug, Clone)]
pub struct StreamingRifeParams {
    pub rife_worker_path: PathBuf,
    pub ffmpeg_path: PathBuf,
    pub model_path: PathBuf,
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
/// Keep this intentionally narrow: the specialized NVEncC path is selected
/// only for Windows + NVIDIA when a requested operation can use a probed
/// specialized capability. Universal remains the default for portable FFmpeg
/// processing and hardware-specific FFmpeg encode backends.
///
/// Graph planning treats AMD as `ExecutorKind::Universal` because no dedicated
/// `ExecutorKind::Amd` variant exists yet. The runtime wraps this in `AmdExecutor`
/// when VCEEncC is available. AMD VCEEncC resize is translated through the
/// planner (via `BackendCapabilityInventory::to_backend_capabilities`) when
/// `--vpp-resize` is probed, routing to `Resize(stage: Executor)`. The runtime
/// family seam then refines that generic Universal resize binding into an
/// explicit AMD-native `AmdVppResize` accelerator annotation before
/// `AmdExecutor` renders the command.
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
        // AmdAi maps to Universal executor kind — the AmdExecutor wrapper is selected
        // at runtime by exec/family.rs based on GPU vendor, not executor preference.
        ExecutorPreference::AmdAi => return ExecutorKind::Universal,
        ExecutorPreference::Auto => {}
    }

    if context.platform == RuntimePlatform::Windows
        && context.gpu_vendor.trim().eq_ignore_ascii_case("nvidia")
        && specialized_nvidia_benefits_request(context.request, context.encoder_capabilities)
    {
        ExecutorKind::NvidiaSpecialized
    } else {
        ExecutorKind::Universal
    }
}

/// Determines whether an NVIDIA-specialized executor is available for the request.
///
/// Returns `true` when the runtime platform is Windows, the `gpu_vendor` string equals
/// `"nvidia"` (case-insensitive, trimming whitespace), and at least one of the encoder
/// capabilities `has_vpp_resize`, `has_fruc`, or `has_truehdr` is enabled; returns `false` otherwise.
///
/// # Examples
///
pub(crate) fn nvidia_ai_available(
    platform: RuntimePlatform,
    gpu_vendor: &str,
    capabilities: &EncoderCapabilities,
) -> bool {
    platform == RuntimePlatform::Windows
        && gpu_vendor.trim().eq_ignore_ascii_case("nvidia")
        && (capabilities.has_vpp_resize || capabilities.has_fruc || capabilities.has_truehdr)
}

/// Determines whether AMD's native VCEEncC AMF FSR resize support is available for the given GPU vendor and encoder capabilities.
///
/// Returns `true` when `gpu_vendor` equals `"amd"` (case-insensitive) and `capabilities.has_vpp_resize` is `true`.
///
/// # Examples
///
pub(crate) fn amd_ai_available(
    _platform: RuntimePlatform,
    gpu_vendor: &str,
    capabilities: &EncoderCapabilities,
) -> bool {
    gpu_vendor.trim().eq_ignore_ascii_case("amd") && capabilities.has_vpp_resize
}

/// Checks whether a `rife-worker` binary exists at the given path.
pub fn detect_rife_capability(rife_path: &Path) -> bool {
    rife_path.is_file()
}

/// Determines whether the request asks for any feature that would benefit from a specialized NVIDIA executor.
///
/// The function returns `true` when the request includes an upscale, frame-generation (interpolation),
/// or HDR transform that matches the NVIDIA-specialized capabilities provided; otherwise returns `false`.
///
/// # Examples
///
fn specialized_nvidia_benefits_request(
    request: &PipelineRequest,
    capabilities: &EncoderCapabilities,
) -> bool {
    requested_upscale_matches_specialized_capabilities(request, capabilities)
        || requested_framegen_matches_specialized_capabilities(request, capabilities)
        || requested_hdr_matches_specialized_capabilities(request, capabilities)
}

fn requested_upscale_matches_specialized_capabilities(
    request: &PipelineRequest,
    capabilities: &EncoderCapabilities,
) -> bool {
    capabilities.has_vpp_resize
        && matches!(
            request.upscale,
            UpscaleRequest::Quality(quality) if (1..=MAX_VSR_QUALITY).contains(&quality)
        )
}

fn requested_framegen_matches_specialized_capabilities(
    request: &PipelineRequest,
    capabilities: &EncoderCapabilities,
) -> bool {
    capabilities.has_fruc
        && request.interpolation == InterpolationRequest::To60
        && !request.source_fps.map(|fps| fps >= 60.0).unwrap_or(false)
}

/// Determine whether the request requires HDR10 tonemapping and the encoder supports HDR transform.
///
/// # Returns
///
/// `true` if the request's HDR mode is `TonemapToHdr10` and `capabilities.has_truehdr` is `true`, `false` otherwise.
///
/// # Examples
///
fn requested_hdr_matches_specialized_capabilities(
    request: &PipelineRequest,
    capabilities: &EncoderCapabilities,
) -> bool {
    capabilities.has_truehdr && request.hdr == HdrRequest::TonemapToHdr10
}

/// Returns `when_available` if `available` is `true`, otherwise `FeatureAvailability::Unavailable`.
///
/// Used to avoid repetitive `if flag { variant } else { Unavailable }` expressions when
/// building `BackendFamilyCapabilities` structs from compact boolean capability flags.
fn feature_availability(
    available: bool,
    when_available: FeatureAvailability,
) -> FeatureAvailability {
    if available {
        when_available
    } else {
        FeatureAvailability::Unavailable
    }
}

impl EncoderCapabilities {
    /// Creates an `EncoderCapabilities` record from a probed `BackendFamilyCapabilities`.
    ///
    /// The returned capabilities reflect only encoder-native features that can be
    /// inferred from the backend probe:
    /// - `has_vpp_resize` is set when the backend family is NVIDIA or AMD and the
    ///   reported `resize` feature is available.
    /// - `has_fruc` and `has_truehdr` are set when the backend family is NVIDIA and
    ///   the corresponding features are available.
    ///
    /// External preprocessing capabilities (portable RIFE and streaming RIFE) are
    /// not derivable from encoder probes and are left `false`; use
    /// `with_rife(...)` to enable it explicitly.
    fn from_backend_capability_profile(profile: &BackendFamilyCapabilities) -> Self {
        Self {
            // The legacy probe shape stays intentionally narrow: it only tracks
            // backend-native flags that historically fed the specialized
            // NVIDIA path and the transitional AMD/VCEEncC residue.
            has_vpp_resize: matches!(profile.family, BackendFamily::Nvidia | BackendFamily::Amd)
                && profile.resize.is_available(),
            has_fruc: matches!(profile.family, BackendFamily::Nvidia)
                && profile.interpolation.is_available(),
            has_truehdr: matches!(profile.family, BackendFamily::Nvidia)
                && profile.hdr_transform.is_available(),
            // Portable RIFE is external-process capability, not encoder-native.
            // It is not derivable from BackendFamilyCapabilities and must be set
            // separately via `with_rife(...)`.
            has_rife: false,
        }
    }

    /// Reconstructs a vendor-native `BackendFamilyCapabilities` from the compact booleans in `EncoderCapabilities`.
    ///
    /// Expands the stored capability flags (`has_vpp_resize`, `has_fruc`, `has_truehdr`) into a full
    /// `BackendFamilyCapabilities` tuned for the given GPU vendor and encoder backend name. Returns
    /// `None` when the vendor is not recognized (i.e., not `"nvidia"` or `"amd"`).
    ///
    /// # Examples
    ///
    fn vendor_native_backend_capabilities(
        &self,
        gpu_vendor: &str,
        encoder_backend: &str,
    ) -> Option<BackendFamilyCapabilities> {
        let backend = encoder_backend.trim().to_ascii_lowercase();
        let vendor = gpu_vendor.trim().to_ascii_lowercase();

        match vendor.as_str() {
            "nvidia" => Some(BackendFamilyCapabilities {
                family: BackendFamily::Nvidia,
                binary_name: "NVEncC".to_string(),
                binary_availability: if backend == "nvenc" {
                    EncoderBinaryAvailability::Available
                } else {
                    EncoderBinaryAvailability::Unknown
                },
                resize: feature_availability(self.has_vpp_resize, FeatureAvailability::Exact),
                interpolation: feature_availability(self.has_fruc, FeatureAvailability::Exact),
                hdr_transform: feature_availability(self.has_truehdr, FeatureAvailability::Exact),
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            "amd" => Some(BackendFamilyCapabilities {
                family: BackendFamily::Amd,
                binary_name: "VCEEncC".to_string(),
                binary_availability: if backend == "vceenc" {
                    EncoderBinaryAvailability::Available
                } else {
                    EncoderBinaryAvailability::Unknown
                },
                resize: feature_availability(self.has_vpp_resize, FeatureAvailability::Approximate),
                interpolation: FeatureAvailability::Unavailable,
                hdr_transform: FeatureAvailability::Unavailable,
                metadata_injection: FeatureAvailability::Unavailable,
            }),
            _ => None,
        }
    }

    /// Returns a capability profile representing the universal (FFmpeg) fallback backend.
    ///
    /// The profile indicates FFmpeg is available and provides exact support for resize,
    /// HDR transform, and metadata injection, while interpolation is unavailable.
    ///
    /// # Examples
    ///
    fn universal_fallback_capabilities() -> BackendFamilyCapabilities {
        BackendFamilyCapabilities {
            family: BackendFamily::Universal,
            binary_name: "ffmpeg".to_string(),
            binary_availability: EncoderBinaryAvailability::Available,
            resize: FeatureAvailability::Exact,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Exact,
            metadata_injection: FeatureAvailability::Exact,
        }
    }

    /// Constructs a BackendFamilyCapabilities profile representing a legacy/unknown encoder
    /// for the given backend name.
    ///
    /// The returned profile marks the backend family as `LegacyCompatibility`, sets the
    /// provided `encoder_backend` string as the `binary_name`, and marks all feature
    /// availability fields as `Unavailable` with `binary_availability` set to `Unknown`.
    ///
    /// # Examples
    ///
    fn legacy_compatibility_backend_capabilities(
        encoder_backend: &str,
    ) -> BackendFamilyCapabilities {
        BackendFamilyCapabilities {
            family: BackendFamily::LegacyCompatibility,
            binary_name: encoder_backend.to_string(),
            binary_availability: EncoderBinaryAvailability::Unknown,
            resize: FeatureAvailability::Unavailable,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        }
    }

    /// Constructs a BackendCapabilityInventory for a pipeline request by selecting an executor kind
    /// and assembling the selected backend plus available vendor-native and universal fallback profiles.
    ///
    /// The selection follows planner rules: the executor kind is chosen via
    /// `select_executor_kind_for_request`, the `selected_backend` is the vendor-native profile when
    /// the chosen executor is `NvidiaSpecialized` (falling back to a legacy compatibility profile if
    /// none), the universal fallback is always included, and vendor-native alternatives exclude the
    /// selected backend family.
    ///
    /// # Returns
    ///
    /// A `BackendCapabilityInventory` containing:
    /// - `selected_executor`: the executor kind chosen for the request,
    /// - `selected_backend`: the backend profile chosen to satisfy the request,
    /// - `vendor_native_backend`: an optional vendor-native alternative profile (excluded if it
    ///   matches the selected backend family),
    /// - `universal_fallback`: the universal (FFmpeg) fallback profile.
    ///
    /// # Examples
    ///
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
        let universal_fallback = Self::universal_fallback_capabilities();
        let vendor_native_backend =
            self.vendor_native_backend_capabilities(gpu_vendor, encoder_backend);
        let selected_backend = match executor {
            ExecutorKind::NvidiaSpecialized => vendor_native_backend.clone().unwrap_or_else(|| {
                Self::legacy_compatibility_backend_capabilities(encoder_backend)
            }),
            ExecutorKind::Universal => universal_fallback.clone(),
            ExecutorKind::Cpu => Self::legacy_compatibility_backend_capabilities(encoder_backend),
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

    /// Derives backend capabilities applicable to a specific request and runtime environment.
    ///
    /// Produces a `BackendCapabilities` value that describes the selected executor target and
    /// the available encoder backend feature set for the provided platform, GPU vendor,
    /// encoder backend, pipeline request, and executor preference.
    ///
    /// # Examples
    ///
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

/// Checks whether the provided help text contains the given option substring.
///
/// This is used to detect the presence of encoder CLI options in a help or usage output.
///
/// # Examples
///
fn help_has_option(help_text: &str, option: &str) -> bool {
    help_text.contains(option)
}

/// Constructs a default "missing" capability profile for a given encoder backend identifier.
///
/// The returned `BackendFamilyCapabilities` represents a probe result when the encoder binary
/// could not be inspected (binary is missing). The function maps common backend identifiers
/// to an appropriate backend family and conservative feature availability values:
/// - `"nvenc"` -> `BackendFamily::Nvidia` (all features unavailable)
/// - `"vceenc"` -> `BackendFamily::Amd` (all features unavailable)
/// - `"ffmpeg"` -> `BackendFamily::Universal` (`resize`/`hdr_transform`/`metadata_injection` set
///   to `Exact`, `interpolation` unavailable)
/// - any other identifier -> `BackendFamily::LegacyCompatibility` (all features unavailable)
///
/// # Examples
///
fn missing_backend_capability_profile(backend: &str) -> BackendFamilyCapabilities {
    match backend.trim().to_ascii_lowercase().as_str() {
        "nvenc" => BackendFamilyCapabilities {
            family: BackendFamily::Nvidia,
            binary_name: "NVEncC".to_string(),
            binary_availability: EncoderBinaryAvailability::Missing,
            resize: FeatureAvailability::Unavailable,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        },
        "vceenc" => BackendFamilyCapabilities {
            family: BackendFamily::Amd,
            binary_name: "VCEEncC".to_string(),
            binary_availability: EncoderBinaryAvailability::Missing,
            resize: FeatureAvailability::Unavailable,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        },
        "ffmpeg" => BackendFamilyCapabilities {
            family: BackendFamily::Universal,
            binary_name: "ffmpeg".to_string(),
            binary_availability: EncoderBinaryAvailability::Missing,
            resize: FeatureAvailability::Exact,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Exact,
            metadata_injection: FeatureAvailability::Exact,
        },
        other => BackendFamilyCapabilities {
            family: BackendFamily::LegacyCompatibility,
            binary_name: other.to_string(),
            binary_availability: EncoderBinaryAvailability::Missing,
            resize: FeatureAvailability::Unavailable,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        },
    }
}

/// Maps an encoder's help text and backend identifier into a BackendFamilyCapabilities profile.
///
/// The function inspects `backend` (case-insensitive) and interprets `help_text` to produce a
/// `BackendFamilyCapabilities` describing the encoder family, binary name, binary availability,
/// and feature availability for resize, interpolation, HDR transform, and metadata injection.
/// Recognized backends:
/// - `"nvenc"` -> Nvidia family (NVEncC), feature flags derived from parsed nvenc help text.
/// - `"vceenc"` -> AMD family (VCEEncC), `--vpp-resize` in `help_text` yields an approximate resize capability.
/// - `"ffmpeg"` -> Universal family (ffmpeg) with resize, HDR transform, and metadata injection available.
/// - any other value -> LegacyCompatibility family with all features marked unavailable.
///
/// # Examples
///
pub(crate) fn parse_encoder_backend_capabilities(
    help_text: &str,
    backend: &str,
) -> BackendFamilyCapabilities {
    match backend.trim().to_ascii_lowercase().as_str() {
        "nvenc" => {
            let legacy = parse_nvenc_capabilities(help_text);
            BackendFamilyCapabilities {
                family: BackendFamily::Nvidia,
                binary_name: "NVEncC".to_string(),
                binary_availability: EncoderBinaryAvailability::Available,
                resize: if legacy.has_vpp_resize {
                    FeatureAvailability::Exact
                } else {
                    FeatureAvailability::Unavailable
                },
                interpolation: if legacy.has_fruc {
                    FeatureAvailability::Exact
                } else {
                    FeatureAvailability::Unavailable
                },
                hdr_transform: if legacy.has_truehdr {
                    FeatureAvailability::Exact
                } else {
                    FeatureAvailability::Unavailable
                },
                metadata_injection: FeatureAvailability::Unavailable,
            }
        }
        "vceenc" => BackendFamilyCapabilities {
            family: BackendFamily::Amd,
            binary_name: "VCEEncC".to_string(),
            binary_availability: EncoderBinaryAvailability::Available,
            resize: if help_has_option(help_text, "--vpp-resize") {
                FeatureAvailability::Approximate
            } else {
                FeatureAvailability::Unavailable
            },
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        },
        "ffmpeg" => BackendFamilyCapabilities {
            family: BackendFamily::Universal,
            binary_name: "ffmpeg".to_string(),
            binary_availability: EncoderBinaryAvailability::Available,
            resize: FeatureAvailability::Exact,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Exact,
            metadata_injection: FeatureAvailability::Exact,
        },
        other => BackendFamilyCapabilities {
            family: BackendFamily::LegacyCompatibility,
            binary_name: other.to_string(),
            binary_availability: EncoderBinaryAvailability::Available,
            resize: FeatureAvailability::Unavailable,
            interpolation: FeatureAvailability::Unavailable,
            hdr_transform: FeatureAvailability::Unavailable,
            metadata_injection: FeatureAvailability::Unavailable,
        },
    }
}

/// Probe an encoder binary's `--help` output and derive its backend capability profile.
///
/// This runs the encoder with `--help`, collects stdout/stderr, and parses the combined help
/// text into a `BackendFamilyCapabilities` profile for the specified backend. If the command
/// cannot be executed, a "missing" capability profile for the backend is returned.
///
/// # Examples
///
pub fn detect_encoder_backend_capabilities(
    encoder_path: &Path,
    backend: &str,
) -> BackendFamilyCapabilities {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    let mut cmd = StdCommand::new(encoder_path);
    cmd.arg("--help");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    let output = cmd.output();

    match output {
        Ok(out) => {
            let mut help_text = String::from_utf8_lossy(&out.stdout).to_string();
            help_text.push_str(&String::from_utf8_lossy(&out.stderr));
            parse_encoder_backend_capabilities(&help_text, backend)
        }
        Err(_) => missing_backend_capability_profile(backend),
    }
}

/// Probes an encoder binary for supported backend features and produces an `EncoderCapabilities` record for planning.

///

/// This invokes backend capability detection for the encoder at `encoder_path` (e.g., by inspecting `--help` output for feature flags)

/// and converts the detected backend capability profile into the planner-facing `EncoderCapabilities`.

///

/// `backend` is the encoder family identifier such as `"nvenc"`, `"vceenc"`, or `"ffmpeg"`; it guides parsing and the shape of the resulting capability profile.

///

/// # Examples

///

pub fn detect_encoder_capabilities(encoder_path: &Path, backend: &str) -> EncoderCapabilities {
    EncoderCapabilities::from_backend_capability_profile(&detect_encoder_backend_capabilities(
        encoder_path,
        backend,
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub source_url: String,
    pub output_url: String,
    pub app_name: Option<String>,
    pub stream_title: Option<String>,
    pub source_content_kind: Option<SourceContentKind>,
    pub upscaler: Option<String>,
    pub source_resolution: Option<String>,
    pub output_resolution: String,
    pub source_fps: Option<f64>,
    pub target_fps: Option<f64>,
    pub framegen_enabled: bool,
    pub hdr_enabled: bool,
    pub quality_level: u8,
    pub executor: ExecutorKind,
    pub encoder_backend: Option<String>,
    /// Client-facing startup flag. Startup is considered complete once the
    /// packager-owned final HLS playlist exists and is non-empty.
    pub startup_complete: bool,
    pub retrying_startup: bool,
    /// Current startup stage for client polling. Values: "starting" | "ready".
    pub startup_stage: String,
}

#[derive(Debug, Clone)]
pub struct PipelineCompletionSignal {
    inner: Arc<PipelineCompletionState>,
}

#[derive(Debug)]
struct PipelineCompletionState {
    complete: AtomicBool,
    notify: tokio::sync::Notify,
}

impl PipelineCompletionSignal {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PipelineCompletionState {
                complete: AtomicBool::new(false),
                notify: tokio::sync::Notify::new(),
            }),
        }
    }

    fn is_complete(&self) -> bool {
        self.inner.complete.load(Ordering::Acquire)
    }

    pub(crate) fn mark_complete(&self) {
        if !self.inner.complete.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    async fn wait(&self) {
        loop {
            if self.is_complete() {
                return;
            }

            let notified = self.inner.notify.notified();
            if self.is_complete() {
                return;
            }
            notified.await;
        }
    }

    async fn wait_timeout(&self, timeout: std::time::Duration) -> bool {
        if self.is_complete() {
            return true;
        }

        tokio::time::timeout(timeout, self.wait()).await.is_ok()
    }
}

impl Default for PipelineCompletionSignal {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Session {
    pub info: SessionInfo,
    pub backing: SessionBacking,
}

#[derive(Debug, Clone)]
pub struct LocalSessionBacking {
    pub source_headers: HashMap<String, String>,
    /// Sending on this channel signals the pipeline task that the client is alive.
    /// Dropping it (via cleanup_session) signals the task to stop the running
    /// pipeline stages, including any inline or standalone packager work.
    pub heartbeat_tx: tokio::sync::mpsc::Sender<()>,
    pub session_dir: PathBuf,
    pub completion: PipelineCompletionSignal,
    pub startup_attempt: u32,
    pub startup_mode: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RemoteSessionBacking {
    pub remote_base_url: String,
    pub remote_session_id: String,
    pub remote_token: String,
    pub selected_peer: RelayPeerMetadata,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SessionBacking {
    Local(LocalSessionBacking),
    Remote(RemoteSessionBacking),
}

impl Session {
    pub fn new_local(
        info: SessionInfo,
        source_headers: HashMap<String, String>,
        heartbeat_tx: tokio::sync::mpsc::Sender<()>,
        session_dir: PathBuf,
        completion: PipelineCompletionSignal,
        startup_attempt: u32,
        startup_mode: impl Into<String>,
    ) -> Self {
        Self {
            info,
            backing: SessionBacking::Local(LocalSessionBacking {
                source_headers,
                heartbeat_tx,
                session_dir,
                completion,
                startup_attempt,
                startup_mode: startup_mode.into(),
            }),
        }
    }

    #[allow(dead_code)]
    pub fn new_remote(
        info: SessionInfo,
        remote_base_url: String,
        remote_session_id: String,
        remote_token: String,
        selected_peer: RelayPeerMetadata,
    ) -> Self {
        Self {
            info,
            backing: SessionBacking::Remote(RemoteSessionBacking {
                remote_base_url,
                remote_session_id,
                remote_token,
                selected_peer,
            }),
        }
    }

    pub fn local_backing(&self) -> Option<&LocalSessionBacking> {
        match &self.backing {
            SessionBacking::Local(local) => Some(local),
            SessionBacking::Remote(_) => None,
        }
    }

    pub fn local_backing_mut(&mut self) -> Option<&mut LocalSessionBacking> {
        match &mut self.backing {
            SessionBacking::Local(local) => Some(local),
            SessionBacking::Remote(_) => None,
        }
    }

    #[allow(dead_code)]
    pub fn remote_backing(&self) -> Option<&RemoteSessionBacking> {
        match &self.backing {
            SessionBacking::Remote(remote) => Some(remote),
            SessionBacking::Local(_) => None,
        }
    }

    pub fn source_headers(&self) -> Option<&HashMap<String, String>> {
        self.local_backing().map(|local| &local.source_headers)
    }

    pub fn session_dir(&self) -> Option<&Path> {
        self.local_backing()
            .map(|local| local.session_dir.as_path())
    }

    pub fn try_send_heartbeat(&self) {
        if let Some(local) = self.local_backing() {
            let _ = local.heartbeat_tx.try_send(());
        }
    }

    pub fn update_local_startup_attempt(&mut self, startup_attempt: u32, startup_mode: String) {
        if let Some(local) = self.local_backing_mut() {
            local.startup_attempt = startup_attempt;
            local.startup_mode = startup_mode;
        }
        self.info.retrying_startup = false;
    }

    pub fn set_retrying_startup(&mut self, retrying: bool) {
        self.info.retrying_startup = retrying;
    }

    pub fn mark_startup_ready(&mut self) {
        self.info.startup_complete = true;
        self.info.retrying_startup = false;
        self.info.startup_stage = "ready".to_string();
    }
}

pub type SessionMap = Arc<DashMap<String, Session>>;

pub fn new_session_map() -> SessionMap {
    Arc::new(DashMap::new())
}

pub(crate) const MAX_VSR_QUALITY: u8 = 4;

pub fn clamp_quality(_resolution: &str, quality: u8) -> u8 {
    quality.clamp(1, MAX_VSR_QUALITY)
}

/// Detects whether encoder startup output contains known transient failure signatures that are safe to retry.
///
/// Checks for specific substrings in `log_output` that indicate early, retryable encoder startup failures (e.g., unreadable input metadata, file reader initialization failure, or AMF encoder hard-failure on Linux).
///
/// # Returns
///
/// `true` if `log_output` contains a known retryable failure signature, `false` otherwise.
///
/// # Examples
///
fn is_retryable_failure(log_output: &str) -> bool {
    let lower = log_output.to_lowercase();
    lower.contains("input video info not parsed yet")
        || lower.contains("failed to initialize file reader")
        // AMF encoder hard-failure on Linux (e.g. RDNA 4 / RADV).  VCEEncC starts
        // up, prints its configuration, then the AMF HEVC engine fails before any
        // output is produced.  The second attempt routes through the universal
        // (VAAPI) fallback via AmdExecutor, so this is safe to retry.
        || lower.contains("break in task amfenc")
}

/// Returns `true` when the pipeline should be retried with a fallback configuration.
///
/// A retry is warranted when the executor stage exited with a non-zero code before the session
/// reached `startup_complete`, the attempt budget has not been exhausted, the backend is
/// known to benefit from a retry (NVIDIA-specialized or VCEEncC), and the stderr output
/// matches a known-transient failure pattern.
fn is_pipeline_retryable(
    exit_code: Option<i32>,
    failed_stage: PipelineStageId,
    startup_complete: bool,
    attempt_number: u32,
    execution_plan: &ExecutionPlan,
    backend_label: &str,
    log: &str,
) -> bool {
    exit_code != Some(0)
        && failed_stage == PipelineStageId::Executor
        && !startup_complete
        && attempt_number < MAX_PIPELINE_START_ATTEMPTS
        && (execution_plan.executor == ExecutorKind::NvidiaSpecialized || backend_label == "vceenc")
        && is_retryable_failure(log)
}

/// Removes all files inside the packager's output directory in preparation for a retry.
///
/// Silently ignores I/O errors for individual file removals so that a partially-written
/// segment does not block the retry path.
async fn cleanup_packager_output(playlist_path: &Path) {
    let parent_dir = playlist_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    if let Ok(mut entries) = tokio::fs::read_dir(&parent_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// Constructs the supervisor timeouts used to drive pipeline lifecycle.
///
/// The returned `PipelineSupervisorTimeouts` contains the inactivity timeout (used after the first
/// heartbeat) and the orphan-session timeout (used while waiting for a first heartbeat), both derived
/// from their corresponding module constants.
///
/// # Examples
///
fn pipeline_supervisor_timeouts() -> PipelineSupervisorTimeouts {
    PipelineSupervisorTimeouts {
        inactivity_timeout: std::time::Duration::from_millis(HEARTBEAT_TIMEOUT_MS),
        orphan_timeout: std::time::Duration::from_millis(ORPHAN_SESSION_TIMEOUT_MS),
    }
}

fn pipeline_cleanup_timeout() -> std::time::Duration {
    std::time::Duration::from_millis(PIPELINE_CLEANUP_TIMEOUT_MS)
}

/// Removes a session from the active session map and logs an info message if a session was removed.
///
/// # Examples
///
fn remove_session_from_pipeline(session_id: &str, sessions: &SessionMap) {
    if sessions.remove(session_id).is_some() {
        debug!(
            "[Pipeline]: Removed session {} from the active session map.",
            session_id
        );
    }
}

/// Remove a session's directory if it exists.
///
/// Attempts to recursively delete `session_dir`. If the directory is already missing,
/// the function returns quietly. Any other I/O error is logged with the provided
/// `session_id` and path.
///
/// # Parameters
///
/// - `session_id`: Identifier used for log messages when removal fails.
/// - `session_dir`: Filesystem path of the session directory to remove.
///
/// # Examples
///
async fn remove_session_dir(session_id: &str, session_dir: &Path) {
    match tokio::fs::remove_dir_all(session_dir).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            warn!(
                "[Pipeline]: Failed to remove session directory for session {} at {}: {}",
                session_id,
                session_dir.display(),
                error
            );
        }
    }
}

async fn finalize_pipeline_session(
    session_id: &str,
    sessions: &SessionMap,
    session_dir: &Path,
    completion: &PipelineCompletionSignal,
) {
    remove_session_from_pipeline(session_id, sessions);
    remove_session_dir(session_id, session_dir).await;
    completion.mark_complete();
}

struct PreparedPipelineLaunch {
    supervisor: PipelineSupervisor,
    startup_mode: String,
}

struct StagedPipelineContexts {
    normalizer: StageRuntimeContext,
    preprocess: Option<StageRuntimeContext>,
    executor: StageRuntimeContext,
    packager: StageRuntimeContext,
}

struct RuntimeExecutorBuildContext<'a> {
    execution_plan: &'a ExecutionPlan,
    intermediate_plan: &'a IntermediateExecutionPlan,
    output_resolution: &'a str,
    profile: &'a EncodingProfile,
    capabilities: &'a EncoderCapabilities,
    denoise: bool,
    gpu_vendor: &'a str,
    encoder_backend: &'a str,
    encoder_path: &'a Path,
    ffmpeg_path: &'a Path,
}

/// Determines whether to attempt a staged (multi-process) pipeline launch for the given source and execution plan.
///
/// Staging is disallowed for HLS-like sources when no local relay is available. For NVIDIA-specialized executors,
/// staging is allowed only when a relay is present. For the universal executor, staging is attempted when the
/// execution plan requires a shared preprocess stage. The CPU (legacy) executor never uses staged launches.
///
/// # Returns
///
/// `true` if a staged pipeline launch should be attempted for the provided source and execution plan, `false` otherwise.
///
/// # Examples
///
fn should_attempt_staged_launch(
    source_descriptor: &SourceDescriptor,
    execution_plan: &ExecutionPlan,
) -> bool {
    // HLS-like sources require the local relay to be running before any staged
    // process can consume from it.  Non-HLS sources (local files, direct HTTP
    // streams, bounded inputs) are consumed by the normalizer stage directly
    // from the source URL and do not need a relay.
    if source_descriptor.classification.is_hls_like() && source_descriptor.relay.is_none() {
        return false;
    }

    match execution_plan.executor {
        // Specialized NVIDIA: temporarily stay on the legacy single-process
        // path even for HLS+relay sources. The staged FFmpeg-normalizer ->
        // NVEncC live handoff is currently unstable (timeline jumps / A-V
        // drift), while the direct NVEncC source-pull path remains the more
        // reliable option for live playback.
        ExecutorKind::NvidiaSpecialized => false,
        // Universal: stage whenever the plan explicitly requires a shared
        // preprocess step, regardless of whether a relay is present.  This
        // makes external RIFE and FFmpeg minterpolate reachable for
        // bounded/file inputs that carry no HLS relay.
        ExecutorKind::Universal => requires_shared_preprocess_stage(execution_plan),
        ExecutorKind::Cpu => false,
    }
}

/// Determines whether the execution plan requires a shared preprocess stage (for work such as interpolation, resize, or HDR)
///
/// Returns `true` if the plan's intermediate representation indicates a separate shared preprocess stage is required, `false` otherwise.
///
/// # Examples
///
fn requires_shared_preprocess_stage(execution_plan: &ExecutionPlan) -> bool {
    execution_plan
        .to_intermediate_plan()
        .shared_preprocess_plan()
        .requires_stage()
}

/// Builds runtime contexts for each stage of a staged (multi-process) pipeline and wires their transports.
///
/// Creates a base stage context and derives four stage-specific contexts:
/// - normalizer: reads from the source and writes to `normalize_transport`.
/// - preprocess: optional; reads from `normalize_transport` and writes to the provided `preprocess_transport`.
/// - executor: reads from either the preprocess output (if present) or `normalize_transport`, and writes to `executor_transport`.
/// - packager: reads from `executor_transport` and writes to the packager output derived from the base context.
///
/// # Returns
///
/// `StagedPipelineContexts` containing the configured `StageRuntimeContext` for `normalizer`, optional `preprocess`,
/// `executor`, and `packager`.
///
/// # Examples
///
fn build_staged_pipeline_contexts(
    session_id: &str,
    execution_plan: &ExecutionPlan,
    source_descriptor: &SourceDescriptor,
    packager_playlist_path: &Path,
    normalize_transport: FrameTransport,
    preprocess_transport: Option<FrameTransport>,
    executor_transport: FrameTransport,
) -> StagedPipelineContexts {
    let base_context = build_stage_context(
        session_id,
        execution_plan,
        source_descriptor,
        packager_playlist_path,
    );
    let packager_output = base_context.transport.output.clone();
    let normalizer_context = StageRuntimeContext {
        transport: TransportConfig {
            input: FrameTransport::SourcePull,
            output: normalize_transport.clone(),
        },
        ..base_context.clone()
    };
    let (preprocess_context, executor_input) = match preprocess_transport {
        Some(preprocess_output) => (
            Some(StageRuntimeContext {
                transport: TransportConfig {
                    input: normalize_transport,
                    output: preprocess_output.clone(),
                },
                ..base_context.clone()
            }),
            preprocess_output,
        ),
        None => (None, normalize_transport),
    };
    let executor_context = StageRuntimeContext {
        transport: TransportConfig {
            input: executor_input,
            output: executor_transport.clone(),
        },
        ..base_context.clone()
    };
    let packager_context = StageRuntimeContext {
        transport: TransportConfig {
            input: executor_transport,
            output: packager_output,
        },
        ..base_context
    };

    StagedPipelineContexts {
        normalizer: normalizer_context,
        preprocess: preprocess_context,
        executor: executor_context,
        packager: packager_context,
    }
}

fn build_stage_context(
    session_id: &str,
    execution_plan: &ExecutionPlan,
    source_descriptor: &SourceDescriptor,
    packager_playlist_path: &Path,
) -> StageRuntimeContext {
    LegacySingleProcessExecutor::build_stage_context(
        session_id,
        execution_plan,
        source_descriptor,
        packager_playlist_path,
    )
}

fn launch_legacy_pipeline_attempt(
    executor: &dyn BackendExecutor,
    session_id: &str,
    backend_label: &str,
    context: &StageRuntimeContext,
    attempt_number: u32,
) -> io::Result<PreparedPipelineLaunch> {
    let startup_mode = LegacyStartupMode::for_attempt(attempt_number);
    let spec = executor.build_executor_spec(context, attempt_number)?;
    let startup_label = spec
        .startup_label
        .as_deref()
        .unwrap_or(startup_mode.as_str());

    info!(
        "[Pipeline:{}]: Launching {} startup path for session {} (attempt {}/{})",
        backend_label, startup_label, session_id, attempt_number, MAX_PIPELINE_START_ATTEMPTS
    );
    debug!(
        "[Pipeline:{}]: Command: {} {}",
        backend_label,
        spec.process.program.display(),
        spec.process.args.join(" ")
    );

    let mut supervisor = PipelineSupervisor::new(session_id, pipeline_supervisor_timeouts());
    supervisor.add_stage(spec.process.spawn_stage_handle()?);

    Ok(PreparedPipelineLaunch {
        supervisor,
        startup_mode: startup_label.to_string(),
    })
}

/// Logs the full command line for a staged pipeline process including backend label and stage.
///
/// # Examples
///
fn log_staged_command(
    backend_label: &str,
    stage: PipelineStageId,
    program: &Path,
    args: &[String],
) {
    debug!(
        "[Pipeline:{}:{}]: Command: {} {}",
        backend_label,
        stage,
        program.display(),
        args.join(" ")
    );
}

/// Selects and builds a preprocess stage ProcessSpec using the best available preprocess backend.
///
/// Tries backends in this order:
/// 1. Streaming RIFE (Python script) — accepts HLS and non-HLS inputs and supports fractional frame rates.
/// 2. Batch RIFE (rife-ncnn-vulkan helper binary) — supports bounded-input preprocessing only.
/// 3. FFmpeg minterpolate fallback.
///
/// The first backend that successfully builds a preprocess spec is returned; if both RIFE options are unavailable or fail to build, an FFmpeg minterpolate spec is constructed and returned.
///
/// # Parameters
///
/// - `ffmpeg_path`: path to the ffmpeg executable used for the fallback minterpolate preprocessor.
/// - `streaming_rife`: optional parameters for a streaming (Python-based) RIFE preprocessor; when provided, streaming RIFE is attempted first.
/// - `context`: runtime stage context used to configure the selected preprocessor spec.
///
/// # Returns
///
/// `Ok(ProcessSpec)` containing a configured preprocess stage specification for the selected backend, or `Err(std::io::Error)` if building the FFmpeg fallback spec fails.
///
/// # Examples
///
fn build_shared_preprocess_spec(
    ffmpeg_path: &Path,
    streaming_rife: Option<&StreamingRifeParams>,
    context: &StageRuntimeContext,
) -> io::Result<crate::runtime::ProcessSpec> {
    // 1. RIFE worker — accepts HLS and non-HLS, supports fractional fps.
    if let Some(params) = streaming_rife {
        let preprocessor = StreamingRifePreprocessor::new(
            &params.rife_worker_path,
            &params.ffmpeg_path,
            &params.model_path,
        )
        .with_output_transport(context.transport.output.clone());
        match preprocessor.build_preprocess_spec(context) {
            Ok(spec) => {
                info!(
                    "[Pipeline:{}]: Using rife-worker preprocess backend (worker={}, model={}).",
                    context.session_id,
                    params.rife_worker_path.display(),
                    params.model_path.display(),
                );
                return Ok(spec);
            }
            Err(error) => {
                warn!(
                    "[Pipeline:{}]: rife-worker preprocess could not be staged: {}. Falling back to FFmpeg minterpolate.",
                    context.session_id, error
                );
            }
        }
    }

    // 2. FFmpeg minterpolate fallback.
    FfmpegPreprocessor::new(ffmpeg_path)
        .with_output_transport(context.transport.output.clone())
        .build_preprocess_spec(context)
}

/// Stops all spawned pipeline stages, awaiting each stage's immediate shutdown.
///
/// The function consumes the provided vector and stops stages in last-in, first-out order
/// by converting each spawn into its handle and calling `stop_now().await`.
///
/// # Examples
///
async fn stop_spawned_stages(mut stage_spawns: Vec<crate::runtime::PipelineStageSpawn>) {
    while let Some(spawn) = stage_spawns.pop() {
        let mut handle = spawn.into_handle();
        handle.stop_now().await;
    }
}

/// Builds and starts a staged multi-process pipeline (normalizer, optional shared preprocess, executor, and packager) and returns a supervisor for the launched stages.
///
/// Attempts to construct each stage's process specification, spawn the processes, and connect their pipes; if any build, spawn, or pipe connection fails the function stops already-spawned stages and returns the encountered I/O error.
///
/// # Returns
///
/// `Ok(PreparedPipelineLaunch)` containing a running `PipelineSupervisor` and the chosen startup mode string on success, `Err(io::Error)` if building or spawning stages or wiring pipes fails.
///
/// # Examples
///
/// Parses a resolution string in "WxH" format into a `(width, height)` tuple.
///
/// Returns `None` if the string cannot be parsed or either dimension is zero.
fn parse_output_resolution(resolution: &str) -> Option<(u32, u32)> {
    let (w_str, h_str) = resolution.split_once('x')?;
    let w = w_str.trim().parse::<u32>().ok()?;
    let h = h_str.trim().parse::<u32>().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

async fn launch_staged_pipeline_attempt(
    executor: &dyn BackendExecutor,
    backend_label: &str,
    session_id: &str,
    source_descriptor: &SourceDescriptor,
    execution_plan: &ExecutionPlan,
    profile: &EncodingProfile,
    packager_playlist_path: &Path,
    attempt_number: u32,
    ffmpeg_path: &Path,
    streaming_rife: Option<&StreamingRifeParams>,
    output_resolution: &str,
    denoise: bool,
) -> io::Result<PreparedPipelineLaunch> {
    let startup_mode = LegacyStartupMode::for_attempt(attempt_number);
    let normalize_transport = FrameTransport::StdoutPipe;
    let preprocess_transport =
        requires_shared_preprocess_stage(execution_plan).then_some(FrameTransport::StdoutPipe);
    let executor_transport = FrameTransport::StdoutPipe;
    let staged_contexts = build_staged_pipeline_contexts(
        session_id,
        execution_plan,
        source_descriptor,
        packager_playlist_path,
        normalize_transport.clone(),
        preprocess_transport,
        executor_transport.clone(),
    );
    // The premium Windows + NVIDIA specialized path stays on NVEncC / Rigaya.
    // Its avsw reader natively stages NV12 frames directly into CUDA memory.
    // The default YUV420P (YV12) forces an extra AVX2 plane-reorder on every
    // frame before GPU upload. Other backends use YUV420P (the
    // widest-compatible planar format) unless they opt in here as well.
    let normalizer_pixel_format = if backend_label == "nvenc" {
        "nv12"
    } else {
        "yuv420p"
    };
    let crop_resolution = parse_output_resolution(output_resolution);
    let normalizer = FfmpegNormalizer::new(ffmpeg_path)
        .with_output_transport(normalize_transport.clone())
        .with_pixel_format(Some(normalizer_pixel_format))
        .with_output_resolution(crop_resolution)
        .with_denoise(denoise);
    let preprocess_spec = staged_contexts
        .preprocess
        .as_ref()
        .map(|context| build_shared_preprocess_spec(ffmpeg_path, streaming_rife, context))
        .transpose()?;
    let packager = FfmpegHlsPackager::new(
        ffmpeg_path,
        FfmpegHlsPackagerContext {
            segment_duration_secs: profile.hls_time,
            list_size: 8,
            delete_segments: true,
            segment_filename_pattern: "segment_%06d.ts".to_string(),
            hls_init_time_secs: 0,
        },
    );
    let normalizer_spec = normalizer.build_normalizer_spec(&staged_contexts.normalizer)?;
    let executor_spec = executor.build_executor_spec(&staged_contexts.executor, attempt_number)?;
    let packager_spec = packager.build_packager_spec(&staged_contexts.packager)?;

    info!(
        "[Pipeline:{}]: Launching {} staged path for session {} (attempt {}/{})",
        backend_label,
        startup_mode.as_str(),
        session_id,
        attempt_number,
        MAX_PIPELINE_START_ATTEMPTS
    );
    log_staged_command(
        backend_label,
        PipelineStageId::Normalizer,
        &normalizer_spec.program,
        &normalizer_spec.args,
    );
    if let Some(spec) = &preprocess_spec {
        log_staged_command(
            backend_label,
            PipelineStageId::Preprocess,
            &spec.program,
            &spec.args,
        );
    }
    log_staged_command(
        backend_label,
        PipelineStageId::Executor,
        &executor_spec.process.program,
        &executor_spec.process.args,
    );
    log_staged_command(
        backend_label,
        PipelineStageId::Packager,
        &packager_spec.program,
        &packager_spec.args,
    );

    let mut stage_spawns = Vec::with_capacity(3 + usize::from(preprocess_spec.is_some()));
    stage_spawns.push(normalizer_spec.spawn_stage()?);

    if let Some(spec) = preprocess_spec {
        match spec.spawn_stage() {
            Ok(spawn) => stage_spawns.push(spawn),
            Err(error) => {
                stop_spawned_stages(stage_spawns).await;
                return Err(error);
            }
        }
    }

    match executor_spec.process.spawn_stage() {
        Ok(spawn) => stage_spawns.push(spawn),
        Err(error) => {
            stop_spawned_stages(stage_spawns).await;
            return Err(error);
        }
    }

    match packager_spec.spawn_stage() {
        Ok(spawn) => stage_spawns.push(spawn),
        Err(error) => {
            stop_spawned_stages(stage_spawns).await;
            return Err(error);
        }
    }

    let mut supervisor = PipelineSupervisor::new(session_id, pipeline_supervisor_timeouts());
    for index in 0..stage_spawns.len().saturating_sub(1) {
        let (upstream, downstream) = stage_spawns.split_at_mut(index + 1);
        if let Err(error) = supervisor.connect_pipe_stages(&mut upstream[index], &mut downstream[0])
        {
            stop_spawned_stages(stage_spawns).await;
            return Err(error);
        }
    }
    for spawn in stage_spawns {
        supervisor.add_stage_spawn(spawn);
    }

    Ok(PreparedPipelineLaunch {
        supervisor,
        startup_mode: startup_mode.as_str().to_string(),
    })
}

/// Selects and constructs a runtime backend executor appropriate for the current platform,
/// GPU vendor, encoder backend, and probed encoder capabilities in the provided build context.
///
/// The selection chooses between specialized NVIDIA, AMD-native, universal FFmpeg, or a
/// legacy single-process executor and returns a boxed executor instance configured with
/// the supplied context (including the intermediate execution plan and encoder paths).
///
/// Returns a boxed `dyn BackendExecutor` implementing the selected runtime executor.
///
/// # Examples
///
fn build_runtime_executor(context: RuntimeExecutorBuildContext<'_>) -> Box<dyn BackendExecutor> {
    debug_assert_eq!(
        context.intermediate_plan.executor,
        context.execution_plan.executor
    );

    let platform = RuntimePlatform::current();
    let executor_target = select_runtime_executor_target(RuntimeExecutorFamilyContext {
        executor_kind: context.intermediate_plan.executor,
        platform,
        gpu_vendor: context.gpu_vendor,
        encoder_backend: context.encoder_backend,
        encoder_capabilities: context.capabilities,
    });

    match executor_target {
        RuntimeExecutorTarget::Family(decision) => match decision.family {
            ExecutorFamily::Nvidia => {
                // Keep the specialized Windows + NVIDIA executor on the existing
                // NVEncC / Rigaya backend-native path for NGX-VSR, FRUC, and
                // TrueHDR support.
                Box::new(NvidiaSpecializedExecutor::new(
                    NvidiaSpecializedExecutorContext {
                        profile: context.profile.clone(),
                        encoder_path: context.encoder_path.to_path_buf(),
                        capabilities: context.capabilities.clone(),
                        intermediate_plan: context.intermediate_plan.clone(),
                        denoise: context.denoise,
                        cas_strength: CAS_DEFAULT_STRENGTH,
                    },
                ))
            }
            ExecutorFamily::Amd => {
                let native_capabilities = decision
                    .amd_capabilities
                    .expect("amd family selection requires amd capabilities");
                Box::new(AmdExecutor::new(AmdExecutorContext {
                    intermediate_plan: context
                        .intermediate_plan
                        .clone()
                        .with_amd_native_resize_bindings(
                            native_capabilities.supports_native_upscale(),
                        ),
                    native_encoder_backend: context.encoder_backend.to_string(),
                    native_encoder_path: context.encoder_path.to_path_buf(),
                    native_capabilities,
                    fallback_executor: build_universal_executor_context(&context, platform),
                    denoise: context.denoise,
                    cas_strength: CAS_DEFAULT_STRENGTH,
                }))
            }
            ExecutorFamily::Universal => Box::new(UniversalExecutor::new(
                build_universal_executor_context(&context, platform),
            )),
        },
        RuntimeExecutorTarget::LegacyCompatibility => Box::new(LegacySingleProcessExecutor::new(
            LegacySingleProcessExecutorContext {
                output_resolution: context.output_resolution.to_string(),
                profile: context.profile.clone(),
                encoder_backend: "vceenc".to_string(),
                encoder_path: context.encoder_path.to_path_buf(),
                capabilities: context.capabilities.clone(),
                denoise: context.denoise,
            },
        )),
    }
}

/// Builds a UniversalExecutorContext from the provided runtime build context and target platform.
///
/// The returned context contains a cloned encoding profile and intermediate plan, the resolved
/// ffmpeg program path, a selected universal encode backend for the given platform and GPU vendor,
/// a fixed VA-API device path (`/dev/dri/renderD128`), and optional shader directory paths
/// (derived from the ffmpeg binary's parent directory) for Anime4K and ArtCNN.
///
/// # Parameters
///
/// - `context`: runtime build context carrying profile, ffmpeg path, intermediate plan, and GPU vendor.
/// - `platform`: target runtime platform used to select the universal encode backend.
fn build_universal_executor_context(
    context: &RuntimeExecutorBuildContext<'_>,
    platform: RuntimePlatform,
) -> UniversalExecutorContext {
    UniversalExecutorContext {
        profile: context.profile.clone(),
        ffmpeg_program: context.ffmpeg_path.to_path_buf(),
        intermediate_plan: context.intermediate_plan.clone(),
        encode_backend: select_universal_encode_backend(UniversalBackendSelectionContext {
            platform,
            gpu_vendor: context.gpu_vendor,
        }),
        vaapi_device: Some(PathBuf::from("/dev/dri/renderD128")),
        anime4k_shaders_dir: context
            .ffmpeg_path
            .parent()
            .map(|p| p.join("shaders").join("anime4k")),
        artcnn_shaders_dir: context
            .ffmpeg_path
            .parent()
            .map(|p| p.join("shaders").join("artcnn")),
        denoise: context.denoise,
        cas_strength: CAS_DEFAULT_STRENGTH,
    }
}

/// Starts and supervises a pipeline for a single session, managing staged vs legacy launches,
/// retries on early encoder failure, and final cleanup of session state and files.
///
/// This function spawns an asynchronous task that:
/// - probes encoder and portable preprocess capabilities (RIFE),
/// - constructs stage/runtime contexts and selects a runtime executor,
/// - attempts a staged multi-process pipeline when appropriate and falls back to a legacy
///   single-process pipeline on failure,
/// - monitors the spawned supervisor for heartbeats, stage exits, and inactivity/orphan timeouts,
/// - implements retry logic for known transient executor failures (with bounded attempts),
/// - marks session startup state, removes the session on terminal failures, and deletes session
///   artifacts before signalling completion.
///
/// # Examples
///
pub fn start_pipeline(
    session_id: &str,
    source_descriptor: &SourceDescriptor,
    output_resolution: &str,
    profile: &EncodingProfile,
    execution_plan: &ExecutionPlan,
    packager_playlist_path: &Path,
    gpu_vendor: &str,
    encoder_backend: &str,
    encoder_path: &Path,
    ffmpeg_path: &Path,
    streaming_rife: Option<StreamingRifeParams>,
    sessions: &SessionMap,
    session_dir: &Path,
    completion: PipelineCompletionSignal,
    hb_rx: tokio::sync::mpsc::Receiver<()>,
) {
    let capabilities = detect_encoder_capabilities(encoder_path, encoder_backend)
        .with_rife(streaming_rife.is_some());
    let ffmpeg_has_hqdn3d = detect_ffmpeg_filter(ffmpeg_path, "hqdn3d");
    info!(
        "[Pipeline:{}]: Capabilities \u{2014} vpp-resize={}, fruc={}, truehdr={}, rife={}, hqdn3d={}",
        encoder_backend,
        capabilities.has_vpp_resize,
        capabilities.has_fruc,
        capabilities.has_truehdr,
        capabilities.has_rife,
        ffmpeg_has_hqdn3d,
    );

    let stage_context = build_stage_context(
        session_id,
        execution_plan,
        source_descriptor,
        packager_playlist_path,
    );
    let intermediate_plan = execution_plan.to_intermediate_plan();
    let executor = build_runtime_executor(RuntimeExecutorBuildContext {
        execution_plan,
        intermediate_plan: &intermediate_plan,
        output_resolution,
        profile,
        capabilities: &capabilities,
        denoise: ffmpeg_has_hqdn3d,
        gpu_vendor,
        encoder_backend,
        encoder_path,
        ffmpeg_path,
    });
    let sessions = sessions.clone();
    let session_id = session_id.to_string();
    let backend_label = encoder_backend.to_string();
    let source_descriptor = source_descriptor.clone();
    let execution_plan = execution_plan.clone();
    let packager_playlist_path = packager_playlist_path.to_path_buf();
    let packager_profile = profile.clone();
    let stage_context = stage_context.clone();
    let session_dir = session_dir.to_path_buf();
    let ffmpeg_path = ffmpeg_path.to_path_buf();
    let streaming_rife = streaming_rife;
    let output_resolution = output_resolution.to_string();

    tokio::spawn(async move {
        let mut attempt_number = 1u32;
        let mut hb_rx = hb_rx;
        let mut allow_staged_launch =
            should_attempt_staged_launch(&source_descriptor, &execution_plan);
        // When the streaming RIFE preprocess stage fails before the session
        // starts up, this flag is set to false so the next attempt falls
        // back to the FFmpeg minterpolate preprocessor.
        let mut use_streaming_rife = streaming_rife.is_some();

        'pipeline: loop {
            let prepared_launch = if allow_staged_launch {
                let effective_streaming_rife = if use_streaming_rife {
                    streaming_rife.as_ref()
                } else {
                    None
                };
                match launch_staged_pipeline_attempt(
                    executor.as_ref(),
                    &backend_label,
                    &session_id,
                    &source_descriptor,
                    &execution_plan,
                    &packager_profile,
                    &packager_playlist_path,
                    attempt_number,
                    &ffmpeg_path,
                    effective_streaming_rife,
                    &output_resolution,
                    ffmpeg_has_hqdn3d,
                )
                .await
                {
                    Ok(launch) => launch,
                    Err(error) => {
                        warn!(
                            "[Pipeline:{}]: Failed to start staged pipeline for session {}: {}; falling back to legacy single-process path.",
                            backend_label,
                            session_id,
                            error
                        );
                        allow_staged_launch = false;
                        match launch_legacy_pipeline_attempt(
                            executor.as_ref(),
                            &session_id,
                            &backend_label,
                            &stage_context,
                            attempt_number,
                        ) {
                            Ok(launch) => launch,
                            Err(e) => {
                                error!(
                                    "[Pipeline:{}]: Failed to spawn encoder for session {}: {}",
                                    backend_label, session_id, e
                                );
                                remove_session_from_pipeline(&session_id, &sessions);
                                break;
                            }
                        }
                    }
                }
            } else {
                // When the plan calls for a shared preprocess stage but we
                // are about to use the single-process legacy path (either
                // because staging was never attempted for this executor/source
                // combination, or because a previous staged attempt failed),
                // preprocess-owned work such as minterpolate or external RIFE
                // interpolation will be silently skipped.  Emit an explicit
                // warning so the loss is visible in logs rather than causing
                // a silent quality regression.
                if requires_shared_preprocess_stage(&execution_plan) {
                    warn!(
                        "[Pipeline:{}]: Session {} using single-process legacy path; \
                         preprocess-owned work (interpolation, resize, HDR) will be skipped.",
                        backend_label, session_id
                    );
                }
                match launch_legacy_pipeline_attempt(
                    executor.as_ref(),
                    &session_id,
                    &backend_label,
                    &stage_context,
                    attempt_number,
                ) {
                    Ok(launch) => launch,
                    Err(e) => {
                        error!(
                            "[Pipeline:{}]: Failed to spawn encoder for session {}: {}",
                            backend_label, session_id, e
                        );
                        remove_session_from_pipeline(&session_id, &sessions);
                        break;
                    }
                }
            };
            let PreparedPipelineLaunch {
                supervisor: launch_supervisor,
                startup_mode,
            } = prepared_launch;
            let mut supervisor = Some(launch_supervisor);

            match sessions.get_mut(&session_id) {
                Some(mut session) => {
                    session.update_local_startup_attempt(attempt_number, startup_mode.clone());
                }
                None => {
                    warn!(
                        "[Pipeline:{}]: Session {} disappeared before process could be registered; aborting.",
                        backend_label,
                        session_id
                    );
                    if let Some(supervisor) = supervisor.take() {
                        let _ = supervisor.run(&mut hb_rx).await;
                    }
                    break;
                }
            }

            let outcome = supervisor
                .take()
                .expect("pipeline supervisor should exist when session remains active")
                .run(&mut hb_rx)
                .await;
            let first_heartbeat_seen = outcome.first_heartbeat_seen;
            let exited_stage = match outcome.stop_reason {
                PipelineSupervisorStopReason::StageExited { stage, .. } => Some(stage),
                _ => None,
            };
            let failed_stage = exited_stage.unwrap_or(PipelineStageId::Executor);
            let failed_report = outcome
                .stage_report(failed_stage)
                .cloned()
                .or_else(|| outcome.stage_report(PipelineStageId::Executor).cloned());
            let exit_code = failed_report.as_ref().and_then(|report| report.exit_code);
            let log = failed_report
                .as_ref()
                .map(|report| report.stderr_tail.clone())
                .unwrap_or_default();
            let stage_label = failed_report
                .as_ref()
                .map(|report| report.stage.to_string())
                .unwrap_or_else(|| failed_stage.to_string());
            let failed_stage_state = failed_report.as_ref().map(|report| report.state);
            let failed_stage_readiness = failed_report.as_ref().map(|report| report.readiness);

            match outcome.stop_reason {
                PipelineSupervisorStopReason::HeartbeatChannelClosed => {
                    break 'pipeline;
                }
                PipelineSupervisorStopReason::OrphanTimeout { waiting_on } => {
                    warn!(
                        "[Pipeline:{}]: Session {} timed out waiting for the first {} heartbeat.",
                        backend_label, session_id, waiting_on
                    );
                    remove_session_from_pipeline(&session_id, &sessions);
                    break 'pipeline;
                }
                PipelineSupervisorStopReason::InactivityTimeout { waiting_on } => {
                    warn!(
                        "[Pipeline:{}]: Session {} timed out due to {} inactivity after the first heartbeat.",
                        backend_label,
                        session_id,
                        waiting_on
                    );
                    remove_session_from_pipeline(&session_id, &sessions);
                    break 'pipeline;
                }
                PipelineSupervisorStopReason::StageExited { stage, exit_code } => {
                    info!(
                        "[Pipeline:{}]: {} stage exited \u{2014} session={} attempt={} exit_code={:?}",
                        backend_label,
                        stage,
                        session_id,
                        attempt_number,
                        exit_code,
                    );
                    debug!(
                        "[Pipeline:{}]: Stage exit detail \u{2014} state={:?} readiness={:?} first_heartbeat={}",
                        backend_label,
                        failed_stage_state,
                        failed_stage_readiness,
                        first_heartbeat_seen
                    );
                }
            }

            if exit_code != Some(0) && !log.is_empty() {
                error!(
                    "[Pipeline:{}:{}]: Last stderr for session {} (attempt {}):\n{}",
                    backend_label,
                    stage_label,
                    session_id,
                    attempt_number,
                    log.trim()
                );
            }

            let (session_exists, startup_complete, should_retry, preprocess_failed) = match sessions
                .get(&session_id)
            {
                Some(session) => {
                    let retryable = is_pipeline_retryable(
                        exit_code,
                        failed_stage,
                        session.info.startup_complete,
                        attempt_number,
                        &execution_plan,
                        &backend_label,
                        &log,
                    );
                    // Also allow a single retry when streaming RIFE preprocess
                    // exits before startup; the retry disables streaming RIFE
                    // so the FFmpeg minterpolate preprocessor is used instead.
                    let preprocess_failed = use_streaming_rife
                        && exit_code != Some(0)
                        && failed_stage == PipelineStageId::Preprocess
                        && !session.info.startup_complete
                        && attempt_number < MAX_PIPELINE_START_ATTEMPTS;
                    if !retryable && !preprocess_failed && exit_code != Some(0) {
                        debug!(
                            "[Pipeline:{}]: No retry \u{2014} session={} stage={} attempt={}/{} startup={} retryable_output={}",
                            backend_label,
                            session_id,
                            failed_stage,
                            attempt_number,
                            MAX_PIPELINE_START_ATTEMPTS,
                            session.info.startup_complete,
                            is_retryable_failure(&log),
                        );
                    }
                    (
                        true,
                        session.info.startup_complete,
                        retryable || preprocess_failed,
                        preprocess_failed,
                    )
                }
                None => {
                    info!(
                        "[Pipeline:{}]: Session {} no longer exists; stopping pipeline loop.",
                        backend_label, session_id
                    );
                    (false, false, false, false)
                }
            };

            if !should_retry {
                if session_exists {
                    if startup_complete {
                        if exit_code == Some(0) {
                            info!(
                                "[Pipeline:{}]: Stream ended \u{2014} session={} (clean exit)",
                                backend_label, session_id,
                            );
                        } else {
                            warn!(
                                "[Pipeline:{}]: Stream ended \u{2014} session={} exit_code={:?}",
                                backend_label, session_id, exit_code,
                            );
                        }
                    } else {
                        warn!(
                            "[Pipeline:{}]: Stream ended before startup \u{2014} session={} exit_code={:?}",
                            backend_label,
                            session_id,
                            exit_code,
                        );
                    }
                    remove_session_from_pipeline(&session_id, &sessions);
                }
                break 'pipeline;
            }

            if preprocess_failed {
                warn!(
                    "[Pipeline:{}]: Streaming RIFE preprocess failed for session {} (attempt {}/{}); \
                     retrying with FFmpeg minterpolate fallback.",
                    backend_label,
                    session_id,
                    attempt_number,
                    MAX_PIPELINE_START_ATTEMPTS,
                );
                use_streaming_rife = false;
            } else {
                warn!(
                    "[Pipeline:{}]: encoder failed before producing output for session {}; retrying with fallback configuration.",
                    backend_label,
                    session_id
                );
            }

            cleanup_packager_output(&packager_playlist_path).await;

            if let Some(mut session) = sessions.get_mut(&session_id) {
                session.set_retrying_startup(true);
            }

            attempt_number += 1;
        }
        finalize_pipeline_session(&session_id, &sessions, &session_dir, &completion).await;
    });
}

/// Clean up and terminate a pipeline session.
///
/// Removes the session entry from `sessions`, signals the running pipeline to stop
/// by dropping its heartbeat sender, waits up to PIPELINE_CLEANUP_TIMEOUT_MS for
/// the pipeline supervisor to finish cleaning up spawned stages, and then removes
/// the session directory. Logs a warning if the supervisor does not finish before
/// the timeout.
///
/// # Parameters
///
/// - `session_id`: Identifier of the session to remove.
/// - `sessions`: Shared session map containing the session to clean up.
///
/// # Examples
///
pub async fn cleanup_session(session_id: &str, sessions: &SessionMap) {
    if let Some((_, session)) = sessions.remove(session_id) {
        debug!("[Pipeline]: Cleaning up session {}...", session_id);
        match session.backing {
            SessionBacking::Local(LocalSessionBacking {
                heartbeat_tx,
                session_dir,
                completion,
                ..
            }) => {
                // Dropping heartbeat_tx signals the pipeline task; completion confirms
                // the supervisor has killed and reaped all children before final cleanup.
                drop(heartbeat_tx);

                if !completion.wait_timeout(pipeline_cleanup_timeout()).await {
                    warn!(
                        "[Pipeline]: Timed out waiting for pipeline task to finish cleaning up session {} after {} ms.",
                        session_id, PIPELINE_CLEANUP_TIMEOUT_MS
                    );
                }

                remove_session_dir(session_id, &session_dir).await;
            }
            SessionBacking::Remote(_) => {
                debug!(
                    "[Pipeline]: Cleaned up remote-backed session {} without local pipeline teardown.",
                    session_id
                );
            }
        }
    }
}

pub enum PlaylistWaitOutcome {
    Ready,
    SessionEnded,
    TimedOut,
}

/// Waits for the packager-owned final HLS playlist file to exist and become
/// non-empty. This remains the session startup readiness gate even when the
/// current backend writes that playlist inline from a legacy single process.
pub async fn wait_for_packager_playlist(
    packager_playlist_path: &Path,
    session_id: &str,
    sessions: &SessionMap,
    max_wait_ms: u64,
) -> PlaylistWaitOutcome {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(max_wait_ms);

    loop {
        if let Ok(meta) = tokio::fs::metadata(packager_playlist_path).await {
            if meta.len() > 0 {
                return PlaylistWaitOutcome::Ready;
            }
        }
        if !sessions.contains_key(session_id) {
            return PlaylistWaitOutcome::SessionEnded;
        }
        if start.elapsed() > timeout {
            return PlaylistWaitOutcome::TimedOut;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_runtime_executor, build_stage_context, build_staged_pipeline_contexts,
        cleanup_session, new_session_map, parse_encoder_backend_capabilities,
        select_executor_kind_for_request, should_attempt_staged_launch, EncoderCapabilities,
        EncodingProfile, ExecutorPreference, ExecutorSelectionContext, PipelineCompletionSignal,
        RuntimeExecutorBuildContext, Session, SessionInfo, FFMPEG_PROGRAM, MAX_VSR_QUALITY,
    };
    use crate::exec::RuntimePlatform;
    use crate::graph::{
        BackendFamily, EncoderBinaryAvailability, ExecutionPlan, ExecutorKind, FeatureAvailability,
        HdrRequest, HdrSupport, InterpolationDecision, InterpolationPlan, InterpolationRequest,
        InterpolationSupport, InterpolationUnsupportedReason, LatencyMode, NativeUpscaleBackend,
        OpExecutionStage, PipelineRequest, ResizePlan, ResizeSupport, SelectedUpscalePath,
        UpscaleRequest, VideoOp,
    };
    use crate::runtime::FrameTransport;
    use crate::settings::RelayPeerMetadata;
    use crate::source::{
        RelayedSource, SourceClassification, SourceContentKind, SourceDescriptor, SourceKind,
        SourceTransport,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use tokio::sync::mpsc;

    fn pipeline_request(
        upscale: UpscaleRequest,
        interpolation: InterpolationRequest,
        hdr: HdrRequest,
    ) -> PipelineRequest {
        PipelineRequest {
            source_transport: SourceTransport::RemoteHttp,
            source_kind: SourceKind::Hls,
            source_content_kind: SourceContentKind::Unknown,
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            latency_mode: LatencyMode::Low,
            upscale,
            interpolation,
            hdr,
        }
    }

    fn unique_session_dir(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("referee-{}-{}", test_name, uuid::Uuid::new_v4()))
    }

    fn cleanup_test_session(
        session_dir: PathBuf,
        completion: PipelineCompletionSignal,
    ) -> (Session, mpsc::Receiver<()>) {
        let (heartbeat_tx, heartbeat_rx) = mpsc::channel(1);
        (
            Session::new_local(
                SessionInfo {
                    id: "session-1".to_string(),
                    source_url: "https://example.com/live/master.m3u8".to_string(),
                    output_url: "http://127.0.0.1:14002/v1/tmp/session-1/index.m3u8".to_string(),
                    app_name: None,
                    stream_title: None,
                    source_content_kind: Some(SourceContentKind::Animated),
                    upscaler: Some("Anime4K2x".to_string()),
                    source_resolution: Some("1920x1080".to_string()),
                    output_resolution: "3840x2160".to_string(),
                    source_fps: Some(30.0),
                    target_fps: Some(30.0),
                    framegen_enabled: false,
                    hdr_enabled: false,
                    quality_level: 3,
                    executor: ExecutorKind::Universal,
                    encoder_backend: Some("universal".to_string()),
                    startup_complete: true,
                    retrying_startup: false,
                    startup_stage: "ready".to_string(),
                },
                HashMap::new(),
                heartbeat_tx,
                session_dir,
                completion,
                1,
                "low-latency",
            ),
            heartbeat_rx,
        )
    }

    fn remote_cleanup_test_session() -> Session {
        Session::new_remote(
            SessionInfo {
                id: "session-1".to_string(),
                source_url: "https://example.com/live/master.m3u8".to_string(),
                output_url: "http://127.0.0.1:14002/v1/tmp/session-1/index.m3u8".to_string(),
                app_name: None,
                stream_title: None,
                source_content_kind: Some(SourceContentKind::Animated),
                upscaler: Some("Anime4K2x".to_string()),
                source_resolution: Some("1920x1080".to_string()),
                output_resolution: "3840x2160".to_string(),
                source_fps: Some(30.0),
                target_fps: Some(30.0),
                framegen_enabled: false,
                hdr_enabled: false,
                quality_level: 3,
                executor: ExecutorKind::Universal,
                encoder_backend: Some("universal".to_string()),
                startup_complete: true,
                retrying_startup: false,
                startup_stage: "ready".to_string(),
            },
            "http://192.168.1.25:14002".to_string(),
            "remote-session-1".to_string(),
            "relay-secret".to_string(),
            RelayPeerMetadata {
                instance_id: Some("peer-1".to_string()),
                hostname: Some("media-box".to_string()),
                ip: Some("192.168.1.25".to_string()),
                version: Some("1.2.3".to_string()),
                platform: Some("linux".to_string()),
                gpu_ready: Some(true),
                gpu_vendor: Some("nvidia".to_string()),
                gpu_name: Some("RTX 4080".to_string()),
            },
        )
    }

    #[tokio::test]
    async fn cleanup_session_drops_heartbeat_and_waits_for_completion() {
        let sessions = new_session_map();
        let session_dir = unique_session_dir("cleanup-waits");
        tokio::fs::create_dir_all(&session_dir).await.unwrap();
        let completion = PipelineCompletionSignal::new();
        let (session, mut heartbeat_rx) =
            cleanup_test_session(session_dir.clone(), completion.clone());
        sessions.insert("session-1".to_string(), session);

        let cleanup = tokio::spawn({
            let sessions = sessions.clone();
            async move {
                cleanup_session("session-1", &sessions).await;
            }
        });

        assert!(heartbeat_rx.recv().await.is_none());
        assert!(!sessions.contains_key("session-1"));
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(!cleanup.is_finished());

        completion.mark_complete();
        tokio::time::timeout(std::time::Duration::from_secs(1), cleanup)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn cleanup_session_returns_when_completion_was_already_marked() {
        let sessions = new_session_map();
        let session_dir = unique_session_dir("cleanup-already-complete");
        tokio::fs::create_dir_all(&session_dir).await.unwrap();
        let completion = PipelineCompletionSignal::new();
        completion.mark_complete();
        let (session, _heartbeat_rx) = cleanup_test_session(session_dir.clone(), completion);
        sessions.insert("session-1".to_string(), session);

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            cleanup_session("session-1", &sessions),
        )
        .await
        .unwrap();

        assert!(!sessions.contains_key("session-1"));
        assert!(tokio::fs::metadata(&session_dir).await.is_err());
    }

    #[tokio::test]
    async fn cleanup_session_removes_temp_dir_only_after_completion() {
        let sessions = new_session_map();
        let session_dir = unique_session_dir("cleanup-temp-dir");
        tokio::fs::create_dir_all(&session_dir).await.unwrap();
        tokio::fs::write(session_dir.join("index.m3u8"), "#EXTM3U")
            .await
            .unwrap();
        let completion = PipelineCompletionSignal::new();
        let (session, mut heartbeat_rx) =
            cleanup_test_session(session_dir.clone(), completion.clone());
        sessions.insert("session-1".to_string(), session);

        let cleanup = tokio::spawn({
            let sessions = sessions.clone();
            async move {
                cleanup_session("session-1", &sessions).await;
            }
        });

        assert!(heartbeat_rx.recv().await.is_none());
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(tokio::fs::metadata(&session_dir).await.is_ok());

        completion.mark_complete();
        tokio::time::timeout(std::time::Duration::from_secs(1), cleanup)
            .await
            .unwrap()
            .unwrap();
        assert!(tokio::fs::metadata(&session_dir).await.is_err());
    }

    #[tokio::test]
    async fn cleanup_session_for_remote_backed_session_returns_without_waiting_for_local_runtime() {
        let sessions = new_session_map();
        sessions.insert("session-1".to_string(), remote_cleanup_test_session());

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            cleanup_session("session-1", &sessions),
        )
        .await
        .unwrap();

        assert!(!sessions.contains_key("session-1"));
    }

    #[test]
    fn executor_selection_uses_specialized_only_for_windows_nvidia_with_matching_feature_request() {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::Off,
            HdrRequest::Off,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::NvidiaSpecialized
        );
    }

    #[test]
    fn executor_selection_uses_specialized_for_windows_nvidia_hdr_or_framegen_benefits() {
        let caps = EncoderCapabilities {
            has_vpp_resize: false,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let framegen_request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::To60,
            HdrRequest::Off,
        );
        let hdr_request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::Off,
            HdrRequest::TonemapToHdr10,
        );

        for request in [&framegen_request, &hdr_request] {
            assert_eq!(
                select_executor_kind_for_request(ExecutorSelectionContext {
                    platform: RuntimePlatform::Windows,
                    gpu_vendor: "nvidia",
                    request,
                    encoder_capabilities: &caps,
                    executor_preference: ExecutorPreference::Auto,
                }),
                ExecutorKind::NvidiaSpecialized
            );
        }
    }

    #[test]
    fn executor_selection_routes_non_beneficial_windows_nvidia_to_universal() {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::Off,
            HdrRequest::Off,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::Universal
        );
    }

    #[test]
    fn executor_selection_nvidia_ai_preference_forces_specialized_when_available() {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::Off,
            HdrRequest::Off,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::NvidiaAi,
            }),
            ExecutorKind::NvidiaSpecialized
        );
    }

    #[test]
    fn executor_selection_nvidia_ai_preference_falls_back_when_unavailable() {
        let caps = EncoderCapabilities {
            has_vpp_resize: false,
            has_fruc: false,
            has_truehdr: false,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::Off,
            HdrRequest::Off,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::NvidiaAi,
            }),
            ExecutorKind::Universal
        );
        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &EncoderCapabilities {
                    has_vpp_resize: true,
                    has_fruc: true,
                    has_truehdr: true,
                    has_rife: false,
                },
                executor_preference: ExecutorPreference::NvidiaAi,
            }),
            ExecutorKind::Universal
        );
    }

    #[test]
    fn executor_selection_requires_requested_feature_to_match_specialized_capability() {
        let caps = EncoderCapabilities {
            has_vpp_resize: false,
            has_fruc: false,
            has_truehdr: false,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::To60,
            HdrRequest::TonemapToHdr10,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::Universal
        );
    }

    #[test]
    fn executor_selection_routes_linux_and_amd_to_universal() {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::To60,
            HdrRequest::TonemapToHdr10,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::Universal
        );
        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Linux,
                gpu_vendor: "amd",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::Universal
        );
        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "amd",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::Universal
        );
    }

    #[test]
    fn executor_selection_universal_preference_overrides_specialized_benefits() {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::To60,
            HdrRequest::TonemapToHdr10,
        );

        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request: &request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Universal,
            }),
            ExecutorKind::Universal
        );
    }

    #[test]
    fn windows_nvidia_capabilities_translate_to_specialized_when_requested_feature_matches() {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::To60,
            HdrRequest::Off,
        );

        let translated = caps.to_backend_capabilities_for_request(
            RuntimePlatform::Windows,
            "nvidia",
            "nvenc",
            &request,
            ExecutorPreference::Auto,
        );

        assert_eq!(translated.executor, ExecutorKind::NvidiaSpecialized);
        assert_eq!(
            translated.resize,
            ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: MAX_VSR_QUALITY,
            }
        );
        assert_eq!(translated.interpolation, InterpolationSupport::To60);
        assert_eq!(
            translated.hdr,
            HdrSupport {
                passthrough_10_bit: false,
                tonemap_to_hdr10: true,
                inject_hdr10_metadata: false,
            }
        );
        assert_eq!(
            translated.upscale.selected_path,
            SelectedUpscalePath::NativeBackend
        );
        assert_eq!(
            translated.upscale.preferred_native_backend,
            Some(NativeUpscaleBackend::NvidiaNgxVsr)
        );
    }

    #[test]
    fn linux_nvidia_capabilities_translate_to_universal_without_claiming_specialized_parity() {
        let caps = EncoderCapabilities {
            has_vpp_resize: false,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Off,
            InterpolationRequest::To60,
            HdrRequest::TonemapToHdr10,
        );

        let translated = caps.to_backend_capabilities_for_request(
            RuntimePlatform::Linux,
            "nvidia",
            "ffmpeg",
            &request,
            ExecutorPreference::Auto,
        );

        assert_eq!(translated.executor, ExecutorKind::Universal);
        assert_eq!(
            translated.resize,
            ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: MAX_VSR_QUALITY,
            }
        );
        assert_eq!(translated.interpolation, InterpolationSupport::Unsupported);
        assert_eq!(
            translated.hdr,
            HdrSupport {
                passthrough_10_bit: true,
                tonemap_to_hdr10: true,
                inject_hdr10_metadata: true,
            }
        );
        assert_eq!(
            translated.upscale.selected_path,
            SelectedUpscalePath::TemporaryUniversalCompatibility
        );
        assert_eq!(translated.upscale.preferred_native_backend, None);
    }

    /// Verifies that when VCEEncC reports `--vpp-resize`, AMD is promoted to the native backend upscale path.
    ///
    /// Asserts that `to_backend_capabilities_for_request` maps an AMD encoder with `--vpp-resize` to:
    /// - `ExecutorKind::Universal` (planner-level selection),
    /// - `ResizeSupport` exposing the VSR quality range,
    /// - `NativeBackend` selected for upscale with `AmdVceEncResize` as the preferred native backend.
    ///
    /// # Examples
    ///
    #[test]
    fn windows_amd_vceenc_with_vpp_resize_promotes_to_native_backend_upscale_path() {
        // When VCEEncC reports --vpp-resize support, AMD is promoted from
        // TemporaryUniversalCompatibility to NativeBackend so the planner
        // routes Resize(Executor) through AmdExecutor's --vpp-resize amf_fsr path.
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: false,
            has_truehdr: false,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::Off,
            HdrRequest::Off,
        );

        let translated = caps.to_backend_capabilities_for_request(
            RuntimePlatform::Windows,
            "amd",
            "vceenc",
            &request,
            ExecutorPreference::Auto,
        );

        assert_eq!(translated.executor, ExecutorKind::Universal);
        assert_eq!(
            translated.resize,
            ResizeSupport::QualityRange {
                min_quality: 1,
                max_quality: MAX_VSR_QUALITY,
            }
        );
        assert_eq!(translated.interpolation, InterpolationSupport::Unsupported);
        assert_eq!(
            translated.hdr,
            HdrSupport {
                passthrough_10_bit: true,
                tonemap_to_hdr10: true,
                inject_hdr10_metadata: true,
            }
        );
        assert_eq!(
            translated.upscale.selected_path,
            SelectedUpscalePath::NativeBackend,
            "AMD with vpp-resize must promote to NativeBackend, not TemporaryUniversalCompatibility"
        );
        assert_eq!(
            translated.upscale.preferred_native_backend,
            Some(NativeUpscaleBackend::AmdVceEncResize)
        );
    }

    /// Constructs a `SourceDescriptor` representing an HLS source that is accessed via a local HTTP relay.
    ///
    /// The descriptor contains both the original source URL and the relay runtime URL, and includes a
    /// populated `relay` field with the same information.
    ///
    /// # Examples
    ///
    fn relayed_source_descriptor() -> SourceDescriptor {
        let relay_url =
            "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8";

        SourceDescriptor {
            classification: SourceClassification {
                transport: SourceTransport::RemoteHttp,
                kind: SourceKind::Hls,
            },
            original_url: "https://example.com/live/master.m3u8".to_string(),
            runtime_url: relay_url.to_string(),
            runtime_headers: HashMap::new(),
            session_headers: HashMap::new(),
            relay: Some(RelayedSource {
                original_url: "https://example.com/live/master.m3u8".to_string(),
                relay_url: relay_url.to_string(),
                headers: HashMap::new(),
                metadata: None,
            }),
            metadata: None,
        }
    }

    fn encoding_profile() -> EncodingProfile {
        EncodingProfile {
            bitrate: 25_000,
            max_bitrate: 37_500,
            preset: "p4".to_string(),
            lookahead: 8,
            bframes: 3,
            hls_time: 1,
        }
    }

    /// Creates an `EncoderCapabilities` with vendor-native features enabled (resize, FRUC, HDR) and RIFE flags disabled.
    ///
    /// The returned value marks `has_vpp_resize`, `has_fruc`, and `has_truehdr` as `true`, and `has_rife` as `false`.
    ///
    /// # Examples
    ///
    fn encoder_capabilities() -> EncoderCapabilities {
        EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: true,
            has_truehdr: true,
            has_rife: false,
        }
    }

    fn executor_plan(executor: ExecutorKind) -> ExecutionPlan {
        ExecutionPlan {
            executor,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        }
    }

    #[test]
    fn generic_capability_parser_only_claims_backend_neutral_features() {
        let help = r#"
VCEEnc (x64) 8.25
   --vpp-resize <string>
   --vpp-fruc [<param1>=<value>]
   --vpp-ngx-truehdr
"#;

        let caps = EncoderCapabilities::from_backend_capability_profile(
            &parse_encoder_backend_capabilities(help, "vceenc"),
        );

        assert!(caps.has_vpp_resize);
        assert!(!caps.has_fruc);
        assert!(!caps.has_truehdr);
    }

    #[test]
    fn backend_capability_parser_distinguishes_nvidia_amd_and_universal_families() {
        let nvenc = parse_encoder_backend_capabilities(
            r#"
Features
ngx: yes
nvof fruc: yes
   --vpp-resize <string>
   --vpp-ngx-truehdr
"#,
            "nvenc",
        );
        let vceenc = parse_encoder_backend_capabilities(
            r#"
VCEEnc (x64) 8.25
   --vpp-resize <string>
"#,
            "vceenc",
        );
        let universal = parse_encoder_backend_capabilities("", "ffmpeg");

        assert_eq!(nvenc.family, BackendFamily::Nvidia);
        assert_eq!(
            nvenc.binary_availability,
            EncoderBinaryAvailability::Available
        );
        assert_eq!(nvenc.resize, FeatureAvailability::Exact);
        assert_eq!(nvenc.interpolation, FeatureAvailability::Exact);
        assert_eq!(nvenc.hdr_transform, FeatureAvailability::Exact);
        assert_eq!(nvenc.metadata_injection, FeatureAvailability::Unavailable);

        assert_eq!(vceenc.family, BackendFamily::Amd);
        assert_eq!(vceenc.resize, FeatureAvailability::Approximate);
        assert_eq!(vceenc.interpolation, FeatureAvailability::Unavailable);
        assert_eq!(vceenc.hdr_transform, FeatureAvailability::Unavailable);

        assert_eq!(universal.family, BackendFamily::Universal);
        assert_eq!(universal.resize, FeatureAvailability::Exact);
        assert_eq!(universal.interpolation, FeatureAvailability::Unavailable);
        assert_eq!(universal.hdr_transform, FeatureAvailability::Exact);
        assert_eq!(universal.metadata_injection, FeatureAvailability::Exact);
    }

    /// Ensures AMD-native backend remains an alternative candidate while the planner selects the Universal executor.
    ///
    /// Verifies that when encoder capabilities indicate only VPP resize for an AMD GPU and the request asks for an upscale quality, the planner selects the Universal executor/backend for runtime but still retains a vendor-native Amd `VCEEncC` candidate in `vendor_native_backend` with `binary_availability = Available`, `resize = Approximate`, and `interpolation`/`hdr_transform` marked unavailable.
    ///
    /// # Examples
    ///
    #[test]
    fn backend_capability_inventory_keeps_amd_native_candidate_separate_from_selected_universal_path(
    ) {
        let caps = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: false,
            has_truehdr: false,
            has_rife: false,
        };
        let request = pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::Off,
            HdrRequest::Off,
        );
        let inventory = caps.to_backend_capability_inventory_for_request(
            RuntimePlatform::Windows,
            "amd",
            "vceenc",
            &request,
            ExecutorPreference::Auto,
        );

        assert_eq!(inventory.selected_executor, ExecutorKind::Universal);
        assert_eq!(inventory.selected_backend.family, BackendFamily::Universal);
        assert_eq!(
            inventory.selected_backend.resize,
            FeatureAvailability::Exact
        );

        let vendor_native = inventory
            .vendor_native_backend
            .expect("amd native backend candidate");
        assert_eq!(vendor_native.family, BackendFamily::Amd);
        assert_eq!(vendor_native.binary_name, "VCEEncC");
        assert_eq!(
            vendor_native.binary_availability,
            EncoderBinaryAvailability::Available
        );
        assert_eq!(vendor_native.resize, FeatureAvailability::Approximate);
        assert_eq!(
            vendor_native.interpolation,
            FeatureAvailability::Unavailable
        );
        assert_eq!(
            vendor_native.hdr_transform,
            FeatureAvailability::Unavailable
        );
    }

    #[test]
    fn runtime_executor_factory_uses_specialized_nvenc_for_specialized_kind() {
        let plan = executor_plan(ExecutorKind::NvidiaSpecialized);
        let intermediate_plan = plan.to_intermediate_plan();
        let profile = encoding_profile();
        let capabilities = encoder_capabilities();
        let executor = build_runtime_executor(RuntimeExecutorBuildContext {
            execution_plan: &plan,
            intermediate_plan: &intermediate_plan,
            output_resolution: "3840x2160",
            profile: &profile,
            capabilities: &capabilities,
            denoise: false,
            gpu_vendor: "nvidia",
            encoder_backend: "nvenc",
            encoder_path: Path::new("NVEncC64.exe"),
            ffmpeg_path: Path::new(FFMPEG_PROGRAM),
        });
        let stage_context = build_stage_context(
            "session-1",
            &plan,
            &relayed_source_descriptor(),
            Path::new("session\\index.m3u8"),
        );

        let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::NvidiaSpecialized);
        assert_eq!(spec.process.program, PathBuf::from("NVEncC64.exe"));
        assert_eq!(spec.process.log_label, "nvenc");
        assert!(spec.process.args.iter().any(|arg| arg == "--multipass"));
    }

    #[test]
    fn runtime_executor_factory_uses_ffmpeg_universal_for_universal_kind() {
        let plan = executor_plan(ExecutorKind::Universal);
        let intermediate_plan = plan.to_intermediate_plan();
        let profile = encoding_profile();
        let capabilities = encoder_capabilities();
        let executor = build_runtime_executor(RuntimeExecutorBuildContext {
            execution_plan: &plan,
            intermediate_plan: &intermediate_plan,
            output_resolution: "3840x2160",
            profile: &profile,
            capabilities: &capabilities,
            denoise: false,
            gpu_vendor: "nvidia",
            encoder_backend: "ffmpeg",
            encoder_path: Path::new("NVEncC64.exe"),
            ffmpeg_path: Path::new(FFMPEG_PROGRAM),
        });
        let stage_context = build_stage_context(
            "session-1",
            &plan,
            &relayed_source_descriptor(),
            Path::new("session\\index.m3u8"),
        );

        let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::Universal);
        assert_eq!(spec.process.program, PathBuf::from(FFMPEG_PROGRAM));
        assert!(spec.process.log_label.starts_with("ffmpeg-universal-"));
        assert!(spec.process.args.iter().any(|arg| arg == "-hide_banner"));
        assert!(!spec.process.args.iter().any(|arg| arg == "--multipass"));
    }

    #[test]
    fn runtime_executor_factory_uses_amd_vceenc_native_resize_when_available() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Low,
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
        let intermediate_plan = plan.to_intermediate_plan();
        let profile = encoding_profile();
        let capabilities = EncoderCapabilities {
            has_vpp_resize: true,
            has_fruc: false,
            has_truehdr: false,
            has_rife: false,
        };
        let executor = build_runtime_executor(RuntimeExecutorBuildContext {
            execution_plan: &plan,
            intermediate_plan: &intermediate_plan,
            output_resolution: "3840x2160",
            profile: &profile,
            capabilities: &capabilities,
            denoise: false,
            gpu_vendor: "amd",
            encoder_backend: "vceenc",
            encoder_path: Path::new("VCEEncC64.exe"),
            ffmpeg_path: Path::new(FFMPEG_PROGRAM),
        });
        let stage_context = build_stage_context(
            "session-1",
            &plan,
            &relayed_source_descriptor(),
            Path::new("session\\index.m3u8"),
        );

        let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::Universal);
        assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
        assert_eq!(spec.process.log_label, "vceenc");
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--output-res", "3840x2160"]),
            "output-res: {:?}",
            spec.process.args
        );
        assert!(
            spec.process
                .args
                .windows(2)
                .any(|w| w == ["--vpp-resize", "amf_fsr"]),
            "vpp-resize: {:?}",
            spec.process.args
        );
        assert!(!spec.process.args.iter().any(|arg| arg == "--vpp-fruc"));
        assert!(!spec
            .process
            .args
            .iter()
            .any(|arg| arg == "--vpp-ngx-truehdr"));
    }

    #[test]
    fn runtime_executor_factory_keeps_amd_on_universal_ffmpeg_path() {
        let plan = executor_plan(ExecutorKind::Universal);
        let intermediate_plan = plan.to_intermediate_plan();
        let profile = encoding_profile();
        let capabilities = encoder_capabilities();
        let executor = build_runtime_executor(RuntimeExecutorBuildContext {
            execution_plan: &plan,
            intermediate_plan: &intermediate_plan,
            output_resolution: "3840x2160",
            profile: &profile,
            capabilities: &capabilities,
            denoise: false,
            gpu_vendor: "amd",
            encoder_backend: "ffmpeg",
            encoder_path: Path::new(FFMPEG_PROGRAM),
            ffmpeg_path: Path::new(FFMPEG_PROGRAM),
        });
        let stage_context = build_stage_context(
            "session-1",
            &plan,
            &relayed_source_descriptor(),
            Path::new("session\\index.m3u8"),
        );

        let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::Universal);
        assert_eq!(spec.process.program, PathBuf::from(FFMPEG_PROGRAM));
        assert!(spec.process.log_label.starts_with("ffmpeg-universal-"));
        assert_ne!(spec.process.log_label, "nvenc");
        assert!(!spec.process.args.iter().any(|arg| arg == "--vpp-fruc"));
    }

    #[test]
    fn runtime_executor_factory_keeps_cpu_on_legacy_inline_hls_compatibility_path() {
        let plan = executor_plan(ExecutorKind::Cpu);
        let intermediate_plan = plan.to_intermediate_plan();
        let profile = encoding_profile();
        let capabilities = encoder_capabilities();
        let executor = build_runtime_executor(RuntimeExecutorBuildContext {
            execution_plan: &plan,
            intermediate_plan: &intermediate_plan,
            output_resolution: "3840x2160",
            profile: &profile,
            capabilities: &capabilities,
            denoise: false,
            gpu_vendor: "unknown",
            encoder_backend: "ffmpeg",
            encoder_path: Path::new(FFMPEG_PROGRAM),
            ffmpeg_path: Path::new(FFMPEG_PROGRAM),
        });
        let stage_context = build_stage_context(
            "session-1",
            &plan,
            &relayed_source_descriptor(),
            Path::new("session\\index.m3u8"),
        );

        let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

        assert_eq!(spec.kind, ExecutorKind::Cpu);
        assert_eq!(spec.process.program, PathBuf::from(FFMPEG_PROGRAM));
        assert_eq!(spec.process.log_label, "vceenc");
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--format", "hls"]));
        assert!(spec
            .process
            .args
            .windows(2)
            .any(|window| window == ["--mux-option", "hls_flags:delete_segments"]));
    }

    #[test]
    fn staged_contexts_insert_optional_preprocess_between_normalizer_and_executor() {
        let execution_plan = ExecutionPlan {
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
            ],
        };
        let contexts = build_staged_pipeline_contexts(
            "session-1",
            &execution_plan,
            &relayed_source_descriptor(),
            Path::new("session\\index.m3u8"),
            FrameTransport::StdoutPipe,
            Some(FrameTransport::StdoutPipe),
            FrameTransport::StdoutPipe,
        );

        let preprocess = contexts.preprocess.expect("preprocess context");
        assert_eq!(
            contexts.normalizer.transport.input,
            FrameTransport::SourcePull
        );
        assert_eq!(
            contexts.normalizer.transport.output,
            FrameTransport::StdoutPipe
        );
        assert_eq!(preprocess.transport.input, FrameTransport::StdoutPipe);
        assert_eq!(preprocess.transport.output, FrameTransport::StdoutPipe);
        assert_eq!(
            contexts.executor.transport.input,
            FrameTransport::StdoutPipe
        );
        assert_eq!(
            contexts.executor.transport.output,
            FrameTransport::StdoutPipe
        );
    }

    #[test]
    fn staged_launch_gate_keeps_nvidia_on_legacy_path_and_enables_universal_when_preprocess_is_required(
    ) {
        let descriptor = relayed_source_descriptor();
        let nvidia_plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        };
        let universal_preprocess_plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Resize(ResizePlan {
                    target_resolution: "2560x1440".to_string(),
                    quality: Some(2),
                    stage: OpExecutionStage::Preprocess,
                }),
            ],
        };
        let universal_legacy_plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        };
        let no_relay_descriptor = SourceDescriptor {
            relay: None,
            ..descriptor.clone()
        };

        assert!(!should_attempt_staged_launch(&descriptor, &nvidia_plan));
        assert!(should_attempt_staged_launch(
            &descriptor,
            &universal_preprocess_plan
        ));
        assert!(!should_attempt_staged_launch(
            &descriptor,
            &universal_legacy_plan
        ));
        // HLS source without relay (edge case): must not stage regardless of
        // executor — the relay server is not running to serve segments.
        assert!(!should_attempt_staged_launch(
            &no_relay_descriptor,
            &nvidia_plan
        ));
        assert!(!should_attempt_staged_launch(
            &no_relay_descriptor,
            &universal_preprocess_plan
        ));
    }

    #[test]
    fn staged_launch_gate_allows_universal_non_hls_sources_when_preprocess_is_required() {
        // A bounded/file/non-HLS source has no relay. Before this fix the relay
        // check blocked ALL non-relay sources from staged launch, making
        // external RIFE and minterpolate interpolation structurally unreachable
        // for local or direct-stream inputs.
        let non_hls_descriptor = SourceDescriptor {
            classification: SourceClassification {
                transport: SourceTransport::LocalPath,
                kind: SourceKind::Other,
            },
            original_url: "/media/video.mkv".to_string(),
            runtime_url: "/media/video.mkv".to_string(),
            runtime_headers: HashMap::new(),
            session_headers: HashMap::new(),
            relay: None,
            metadata: None,
        };

        // Non-HLS Universal + preprocess-owned interpolation: staged launch
        // must now be attempted so the preprocess stage can run RIFE/minterpolate.
        let universal_interpolation_plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: false,
            video_ops: vec![
                VideoOp::NormalizeInput,
                VideoOp::Interpolate(InterpolationPlan {
                    target_fps: 60,
                    decision: InterpolationDecision::portable_fallback(
                        OpExecutionStage::Preprocess,
                    ),
                }),
            ],
        };
        // Non-HLS Universal + no preprocess: legacy path is fine, no staging needed.
        let universal_no_preprocess_plan = ExecutionPlan {
            executor: ExecutorKind::Universal,
            latency_mode: LatencyMode::Balanced,
            requires_local_hls_relay: false,
            video_ops: vec![VideoOp::NormalizeInput],
        };
        // Non-HLS NVIDIA: preserve existing behavior — staged only for HLS+relay.
        let nvidia_plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: false,
            video_ops: vec![VideoOp::NormalizeInput],
        };

        assert!(
            should_attempt_staged_launch(&non_hls_descriptor, &universal_interpolation_plan),
            "non-HLS Universal with preprocess-owned interpolation must now stage"
        );
        assert!(
            !should_attempt_staged_launch(&non_hls_descriptor, &universal_no_preprocess_plan),
            "non-HLS Universal without preprocess does not need staged launch"
        );
        assert!(
            !should_attempt_staged_launch(&non_hls_descriptor, &nvidia_plan),
            "non-HLS NVIDIA must keep existing legacy-only behavior (no relay)"
        );
    }

    #[test]
    fn is_retryable_failure_recognizes_known_encoder_startup_errors() {
        assert!(super::is_retryable_failure(
            "Error: input video info not parsed yet"
        ));
        assert!(super::is_retryable_failure(
            "ERROR: Failed to initialize file reader"
        ));
        // AMF encoder hard-failure on Linux (RDNA 4 / RADV).
        assert!(super::is_retryable_failure(
            "\x1b[31mBreak in task AMFENC: unknown error..\x1b[39m"
        ));
        assert!(!super::is_retryable_failure("ERROR: out of memory"));
        assert!(!super::is_retryable_failure(""));
    }

    #[test]
    fn planned_target_frame_rate_returns_native_interpolation_target_fps() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::Interpolate(InterpolationPlan {
                target_fps: 48,
                decision: InterpolationDecision::native_backend(OpExecutionStage::Executor),
            })],
        };

        assert_eq!(
            super::planned_target_frame_rate(Some(24.0), &plan),
            Some(48.0)
        );
    }

    #[test]
    fn planned_target_frame_rate_keeps_source_fps_for_portable_interpolation_gap() {
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

        assert!(!super::is_interpolation_enabled(&plan));
        assert_eq!(
            super::planned_target_frame_rate(Some(30.0), &plan),
            Some(30.0)
        );
    }

    #[test]
    fn planned_target_frame_rate_passes_through_source_fps_when_interpolation_is_off() {
        let plan = ExecutionPlan {
            executor: ExecutorKind::NvidiaSpecialized,
            latency_mode: LatencyMode::Low,
            requires_local_hls_relay: true,
            video_ops: vec![VideoOp::NormalizeInput],
        };

        assert_eq!(
            super::planned_target_frame_rate(Some(24.0), &plan),
            Some(24.0)
        );
        assert_eq!(super::planned_target_frame_rate(None, &plan), None);
    }
}
