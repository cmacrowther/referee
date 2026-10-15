use serde::{Deserialize, Serialize};

use crate::source::{SourceContentKind, SourceKind, SourceTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LatencyMode {
    UltraLow,
    #[default]
    Low,
    Balanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum UpscaleRequest {
    #[default]
    Off,
    Quality(u8),
    Anime4k2x,
    Artcnn2x,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InterpolationRequest {
    #[default]
    Off,
    /// Generate frames up to this output frame rate.
    TargetFps(u32),
    /// Generate frames up to this output frame rate with RIFE, in the
    /// preprocess stage (`rife-worker`), on any executor.
    RifeTargetFps(u32),
    /// Double the source frame rate with the executor's native frame
    /// generation (NVIDIA Framegen or AMD Fluid Motion), which only multiplies
    /// by whole numbers.
    NativeDouble,
}

/// Fraction of the target a source rate must reach to count as already at the
/// target, so NTSC rates (29.97, 59.94) are not interpolated to 30 or 60.
const TARGET_FPS_TOLERANCE: f64 = 0.99;
/// Doubling skips sources already at this rate: they are smooth, and doubling
/// them costs more than it gains.
const DOUBLING_SOURCE_FPS_LIMIT: u32 = 60;
/// Source rate assumed when doubling a source whose frame rate is unknown.
const UNKNOWN_SOURCE_FPS_FOR_DOUBLING: f64 = 30.0;

impl InterpolationRequest {
    /// The output frame rate interpolation produces for a source at
    /// `source_fps`.
    pub fn target_fps(self, source_fps: Option<f64>) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::TargetFps(fps) | Self::RifeTargetFps(fps) => Some(fps),
            Self::NativeDouble => {
                let source_fps = source_fps
                    .filter(|fps| *fps > 0.0)
                    .unwrap_or(UNKNOWN_SOURCE_FPS_FOR_DOUBLING);
                Some((source_fps * 2.0).round() as u32)
            }
        }
    }

    /// Whether interpolation should add frames: it is requested and the source
    /// is not already at the target (or, when doubling, at 60 fps). An unknown
    /// source rate counts as below the target.
    pub fn raises_frame_rate(self, source_fps: Option<f64>) -> bool {
        let threshold = match self {
            Self::Off => return false,
            Self::TargetFps(fps) | Self::RifeTargetFps(fps) => fps,
            Self::NativeDouble => DOUBLING_SOURCE_FPS_LIMIT,
        };
        !source_fps.is_some_and(|fps| source_reaches_target_fps(fps, threshold))
    }
}

/// Whether `source_fps` is at or above `target_fps`, within
/// [`TARGET_FPS_TOLERANCE`].
pub fn source_reaches_target_fps(source_fps: f64, target_fps: u32) -> bool {
    source_fps >= target_fps as f64 * TARGET_FPS_TOLERANCE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HdrRequest {
    #[default]
    Off,
    Passthrough10Bit,
    TonemapToHdr10,
    InjectHdr10Metadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRequest {
    pub source_transport: SourceTransport,
    pub source_kind: SourceKind,
    #[serde(default)]
    pub source_content_kind: SourceContentKind,
    pub source_resolution: Option<String>,
    pub output_resolution: String,
    pub source_fps: Option<f64>,
    pub latency_mode: LatencyMode,
    pub upscale: UpscaleRequest,
    pub interpolation: InterpolationRequest,
    pub hdr: HdrRequest,
}

impl PipelineRequest {
    pub fn requires_local_hls_relay(&self) -> bool {
        matches!(self.source_transport, SourceTransport::RemoteHttp)
            && matches!(self.source_kind, SourceKind::Hls)
    }
}
