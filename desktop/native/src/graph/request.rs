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
    To60,
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
