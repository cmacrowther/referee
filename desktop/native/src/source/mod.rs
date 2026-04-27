pub mod hls_relay;

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

const REQUEST_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const STREAM_PROBE_TIMEOUT_SECS: u64 = 12;

static HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(REQUEST_USER_AGENT)
        .timeout(std::time::Duration::from_secs(STREAM_PROBE_TIMEOUT_SECS))
        .build()
        .expect("Failed to build HTTP client")
});
static STREAM_INF_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"#EXT-X-STREAM-INF:([^\n]+)").unwrap());
static RES_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"RESOLUTION=(\d+)x(\d+)").unwrap());
static FPS_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"FRAME-RATE=([\d.]+)").unwrap());
static BW_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"BANDWIDTH=(\d+)").unwrap());
const CONTENT_PROBE_TIMEOUT_SECS: u64 = 8;
const CONTENT_PROBE_WIDTH: usize = 64;
const CONTENT_PROBE_HEIGHT: usize = 36;
const CONTENT_PROBE_FRAMES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceTransport {
    RemoteHttp,
    LocalFile,
    LocalPath,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Hls,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceContentKind {
    Animated,
    LiveAction,
    #[default]
    Unknown,
}

impl SourceContentKind {
    pub fn log_label(self) -> &'static str {
        match self {
            Self::Animated => "animation",
            Self::LiveAction => "live action",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceClassification {
    pub transport: SourceTransport,
    pub kind: SourceKind,
}

impl SourceClassification {
    pub fn is_remote_http(self) -> bool {
        matches!(self.transport, SourceTransport::RemoteHttp)
    }

    pub fn is_hls_like(self) -> bool {
        matches!(self.kind, SourceKind::Hls)
    }

    pub fn requires_local_hls_relay(self) -> bool {
        self.is_remote_http() && self.is_hls_like()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMetadata {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_resolution: Option<String>,
    pub source_fps: Option<f64>,
    #[serde(default)]
    pub content_kind: SourceContentKind,
    pub content_kind_confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SourceContentProbe {
    kind: SourceContentKind,
    confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDescriptor {
    pub classification: SourceClassification,
    pub original_url: String,
    pub runtime_url: String,
    pub runtime_headers: HashMap<String, String>,
    pub session_headers: HashMap<String, String>,
    pub relay: Option<hls_relay::RelayedSource>,
    pub metadata: Option<SourceMetadata>,
}

pub fn classify_source_url(source_url: &str) -> SourceClassification {
    let transport = match reqwest::Url::parse(source_url) {
        Ok(url) => match url.scheme() {
            "http" | "https" => SourceTransport::RemoteHttp,
            "file" => SourceTransport::LocalFile,
            _ => SourceTransport::Other,
        },
        Err(_) if Path::new(source_url).is_absolute() => SourceTransport::LocalPath,
        Err(_) => SourceTransport::Other,
    };

    let kind = if is_hls_like_input(source_url) {
        SourceKind::Hls
    } else {
        SourceKind::Other
    };

    SourceClassification { transport, kind }
}

pub async fn probe_source_metadata(
    source_url: &str,
    headers: &HashMap<String, String>,
) -> Option<SourceMetadata> {
    let classification = classify_source_url(source_url);
    probe_source_metadata_with_classification(source_url, headers, classification, None).await
}

pub async fn describe_source(
    relay_host: &str,
    relay_port: u16,
    session_id: &str,
    original_url: &str,
    headers: &HashMap<String, String>,
    ffprobe_path: Option<&std::path::Path>,
) -> SourceDescriptor {
    let classification = classify_source_url(original_url);
    let metadata = probe_source_metadata_with_classification(
        original_url,
        headers,
        classification,
        ffprobe_path,
    )
    .await;

    assemble_source_descriptor(
        relay_host,
        relay_port,
        session_id,
        original_url,
        headers,
        classification,
        metadata,
    )
}

fn assemble_source_descriptor(
    relay_host: &str,
    relay_port: u16,
    session_id: &str,
    original_url: &str,
    headers: &HashMap<String, String>,
    classification: SourceClassification,
    metadata: Option<SourceMetadata>,
) -> SourceDescriptor {
    let relay = hls_relay::describe_relayed_source(
        relay_host,
        relay_port,
        session_id,
        original_url,
        headers,
    );
    let runtime_url = relay
        .as_ref()
        .map(|relayed| relayed.relay_url.clone())
        .unwrap_or_else(|| original_url.to_string());
    let runtime_headers = if relay.is_some() {
        HashMap::new()
    } else {
        headers.clone()
    };

    SourceDescriptor {
        classification,
        original_url: original_url.to_string(),
        runtime_url,
        runtime_headers,
        session_headers: headers.clone(),
        relay,
        metadata,
    }
}

#[allow(unused_imports)]
pub use hls_relay::{describe_relayed_source, relay_request, should_relay_input, RelayedSource};

// ── SSRF protection ──────────────────────────────────────────────────────────

/// Returns `true` when the URL's resolved address should be blocked to prevent SSRF.
///
/// Blocks loopback (127.0.0.0/8, ::1) and link-local (169.254.0.0/16, fe80::/10)
/// ranges. RFC-1918 private ranges are intentionally *not* blocked because legitimate
/// LAN stream sources such as a NAS or another media PC must remain reachable.
pub async fn is_ssrf_target(url: &str) -> bool {
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) => u,
        // Not a parseable URL — the scheme validation in validate_stream_start_request
        // will catch this separately; here we do not need to block it.
        Err(_) => return false,
    };

    let host = match parsed.host_str() {
        Some(h) => h.to_string(),
        None => return true, // no host — cannot safely proceed
    };

    // If the host is already a literal IP, check it directly without DNS.
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_blocked_ip(ip);
    }

    // Resolve via DNS; block if resolution fails or *any* returned address is blocked.
    let port = parsed.port_or_known_default().unwrap_or(80);
    match tokio::net::lookup_host(format!("{}:{}", host, port)).await {
        Ok(addrs) => {
            let addrs: Vec<_> = addrs.collect();
            // An empty result (no A/AAAA records) also means we cannot proceed safely.
            if addrs.is_empty() {
                return true;
            }
            addrs.iter().any(|a| is_blocked_ip(a.ip()))
        }
        Err(_) => true, // DNS failure — fail closed
    }
}

fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                // fe80::/10 — link-local (is_unicast_link_local is not yet stable)
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

async fn probe_source_metadata_with_classification(
    source_url: &str,
    headers: &HashMap<String, String>,
    classification: SourceClassification,
    ffprobe_path: Option<&std::path::Path>,
) -> Option<SourceMetadata> {
    if !classification.requires_local_hls_relay() {
        info!("[Metadata Probe]: Source is not a remote HLS input; skipping manifest probe.");
        return None;
    }

    let mut request = HTTP_CLIENT.get(source_url);
    for (key, value) in headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let body = match request.send().await {
        Ok(response) => match response.text().await {
            Ok(text) => text,
            Err(error) => {
                warn!(
                    "[Metadata Probe]: Failed to read HLS manifest body: {}",
                    error
                );
                return None;
            }
        },
        Err(error) => {
            warn!("[Metadata Probe]: Failed to fetch HLS manifest: {}", error);
            return None;
        }
    };

    // Fast path: manifest probe works for master playlists with STREAM-INF attributes.
    let metadata = if let Some(metadata) = parse_hls_manifest(&body) {
        Some(metadata)
    } else {
        // Fallback: variant/media playlists have no RESOLUTION/FRAME-RATE tags.
        // Use ffprobe to interrogate the stream directly.
        info!("[Metadata Probe]: HLS manifest has no stream-inf entries; falling back to ffprobe.");
        probe_with_ffprobe(source_url, headers, ffprobe_path).await
    };

    attach_content_probe(
        metadata,
        probe_source_content_kind(source_url, headers, ffprobe_path).await,
    )
}

fn is_hls_like_input(source_url: &str) -> bool {
    let lower = source_url.to_ascii_lowercase();
    lower.contains(".m3u8") || lower.contains(".m3u")
}

fn parse_hls_manifest(body: &str) -> Option<SourceMetadata> {
    let mut best_width = None;
    let mut best_height = None;
    let mut best_bandwidth: u64 = 0;
    let mut best_fps = None;

    for caps in STREAM_INF_RE.captures_iter(body) {
        let attrs = &caps[1];
        let bandwidth = BW_RE
            .captures(attrs)
            .and_then(|captures| captures[1].parse::<u64>().ok())
            .unwrap_or(0);

        if let Some(resolution_caps) = RES_RE.captures(attrs) {
            if bandwidth >= best_bandwidth {
                best_width = resolution_caps[1].parse().ok();
                best_height = resolution_caps[2].parse().ok();
                best_bandwidth = bandwidth;
                if let Some(fps_caps) = FPS_RE.captures(attrs) {
                    best_fps = fps_caps[1].parse::<f64>().ok().map(round_frame_rate);
                }
            }
        }
    }

    if best_width.is_none() && best_height.is_none() && best_fps.is_none() {
        return None;
    }

    let source_resolution = match (best_width, best_height) {
        (Some(width), Some(height)) => Some(format!("{}x{}", width, height)),
        _ => None,
    };

    info!(
        "[Metadata Probe]: Parsed HLS manifest - {}, {} fps",
        source_resolution.as_deref().unwrap_or("unknown resolution"),
        best_fps
            .map(|fps| fps.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );

    Some(SourceMetadata {
        width: best_width,
        height: best_height,
        source_resolution,
        source_fps: best_fps,
        content_kind: SourceContentKind::Unknown,
        content_kind_confidence: None,
    })
}

fn round_frame_rate(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn parse_rational_fps(value: &str) -> Option<f64> {
    let (num_str, den_str) = value.split_once('/')?;
    let num: f64 = num_str.trim().parse().ok()?;
    let den: f64 = den_str.trim().parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some(round_frame_rate(num / den))
}

async fn probe_with_ffprobe(
    source_url: &str,
    headers: &HashMap<String, String>,
    ffprobe_path: Option<&std::path::Path>,
) -> Option<SourceMetadata> {
    let ffprobe_bin: &std::ffi::OsStr = ffprobe_path
        .map(|p| p.as_os_str())
        .unwrap_or_else(|| std::ffi::OsStr::new("ffprobe"));
    let mut command = tokio::process::Command::new(ffprobe_bin);
    command.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,r_frame_rate",
        "-of",
        "default=noprint_wrappers=1",
    ]);
    if !headers.is_empty() {
        command.arg("-headers").arg(build_input_headers(headers));
    }
    command.arg(source_url);
    crate::process::hide_tokio_command_window(&mut command);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output()).await;

    let stdout = match output {
        Ok(Ok(out)) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        Ok(Ok(out)) => {
            warn!(
                "[Metadata Probe]: ffprobe exited with error: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            return None;
        }
        Ok(Err(error)) => {
            warn!("[Metadata Probe]: Failed to run ffprobe: {}", error);
            return None;
        }
        Err(_) => {
            warn!("[Metadata Probe]: ffprobe timed out after 10s");
            return None;
        }
    };

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut source_fps: Option<f64> = None;

    for line in stdout.lines() {
        if let Some(val) = line.strip_prefix("width=") {
            width = val.parse().ok();
        } else if let Some(val) = line.strip_prefix("height=") {
            height = val.parse().ok();
        } else if let Some(val) = line.strip_prefix("r_frame_rate=") {
            source_fps = parse_rational_fps(val);
        }
    }

    if width.is_none() && height.is_none() && source_fps.is_none() {
        return None;
    }

    let source_resolution = match (width, height) {
        (Some(w), Some(h)) => Some(format!("{}x{}", w, h)),
        _ => None,
    };

    info!(
        "[Metadata Probe]: ffprobe result — {}, {:?} fps",
        source_resolution.as_deref().unwrap_or("unknown resolution"),
        source_fps
    );

    Some(SourceMetadata {
        width,
        height,
        source_resolution,
        source_fps,
        content_kind: SourceContentKind::Unknown,
        content_kind_confidence: None,
    })
}

fn attach_content_probe(
    metadata: Option<SourceMetadata>,
    probe: Option<SourceContentProbe>,
) -> Option<SourceMetadata> {
    match (metadata, probe) {
        (Some(mut metadata), Some(probe)) => {
            metadata.content_kind = probe.kind;
            metadata.content_kind_confidence = Some(probe.confidence);
            Some(metadata)
        }
        (Some(metadata), None) => Some(metadata),
        (None, Some(probe)) => Some(SourceMetadata {
            content_kind: probe.kind,
            content_kind_confidence: Some(probe.confidence),
            ..SourceMetadata::default()
        }),
        (None, None) => None,
    }
}

async fn probe_source_content_kind(
    source_url: &str,
    headers: &HashMap<String, String>,
    ffprobe_path: Option<&std::path::Path>,
) -> Option<SourceContentProbe> {
    let ffmpeg_bin = ffprobe_path
        .map(|path| {
            path.with_file_name(if cfg!(target_os = "windows") {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            })
        })
        .unwrap_or_else(|| {
            std::path::PathBuf::from(if cfg!(target_os = "windows") {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            })
        });
    let scale_filter = format!(
        "scale={}:{}:flags=fast_bilinear,format=rgb24",
        CONTENT_PROBE_WIDTH, CONTENT_PROBE_HEIGHT
    );
    let frame_limit = CONTENT_PROBE_FRAMES.to_string();
    let mut command = tokio::process::Command::new(ffmpeg_bin);
    command.args([
        "-v",
        "error",
        "-nostdin",
        "-analyzeduration",
        "1000000",
        "-probesize",
        "1000000",
        "-reconnect",
        "1",
        "-reconnect_streamed",
        "1",
        "-reconnect_on_network_error",
        "1",
        "-reconnect_delay_max",
        "5",
    ]);
    if !headers.is_empty() {
        command.arg("-headers").arg(build_input_headers(headers));
    }
    command
        .arg("-i")
        .arg(source_url)
        .arg("-an")
        .arg("-sn")
        .arg("-dn")
        .arg("-frames:v")
        .arg(&frame_limit)
        .arg("-vf")
        .arg(&scale_filter)
        .arg("-f")
        .arg("rawvideo")
        .arg("pipe:1");
    crate::process::hide_tokio_command_window(&mut command);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(CONTENT_PROBE_TIMEOUT_SECS),
        command.output(),
    )
    .await;

    let stdout = match output {
        Ok(Ok(out)) if out.status.success() => out.stdout,
        Ok(Ok(out)) => {
            warn!(
                "[Content Probe]: ffmpeg exited with error: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            return None;
        }
        Ok(Err(error)) => {
            warn!("[Content Probe]: Failed to run ffmpeg: {}", error);
            return None;
        }
        Err(_) => {
            warn!(
                "[Content Probe]: ffmpeg timed out after {}s",
                CONTENT_PROBE_TIMEOUT_SECS
            );
            return None;
        }
    };

    let probe = classify_rgb24_frames(&stdout, CONTENT_PROBE_WIDTH, CONTENT_PROBE_HEIGHT);

    if let Some(probe) = probe {
        info!(
            "[Content Probe]: Classified source as {} (confidence {:.0}%) from {} sampled frames at {}x{}: {}",
            probe.kind.log_label(),
            probe.confidence * 100.0,
            CONTENT_PROBE_FRAMES,
            CONTENT_PROBE_WIDTH,
            CONTENT_PROBE_HEIGHT,
            source_url
        );
        Some(probe)
    } else {
        warn!(
            "[Content Probe]: Could not classify source content from sampled frames: {}",
            source_url
        );
        None
    }
}

fn classify_rgb24_frames(
    rgb_samples: &[u8],
    width: usize,
    height: usize,
) -> Option<SourceContentProbe> {
    let frame_size = width.checked_mul(height)?.checked_mul(3)?;
    if frame_size == 0 || rgb_samples.len() < frame_size {
        return None;
    }

    let pixel_count = width * height;
    let mut frame_count = 0usize;
    let mut flat_ratio_total = 0.0f32;
    let mut mid_ratio_total = 0.0f32;
    let mut strong_ratio_total = 0.0f32;
    let mut low_variance_ratio_total = 0.0f32;
    let mut palette_ratio_total = 0.0f32;

    for frame in rgb_samples.chunks_exact(frame_size) {
        let mut luma = vec![0u8; pixel_count];
        let mut bins = [false; 4096];
        let mut unique_bins = 0usize;

        for pixel_index in 0..pixel_count {
            let base = pixel_index * 3;
            let r = frame[base] as u16;
            let g = frame[base + 1] as u16;
            let b = frame[base + 2] as u16;
            luma[pixel_index] = ((77 * r + 150 * g + 29 * b) >> 8) as u8;

            let bin = (((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4)) as usize;
            if !bins[bin] {
                bins[bin] = true;
                unique_bins += 1;
            }
        }

        let mut flat_gradients = 0usize;
        let mut mid_gradients = 0usize;
        let mut strong_gradients = 0usize;
        let mut gradient_samples = 0usize;

        for y in 0..height.saturating_sub(1) {
            for x in 0..width.saturating_sub(1) {
                let index = y * width + x;
                let gx = u8::abs_diff(luma[index], luma[index + 1]) as u16;
                let gy = u8::abs_diff(luma[index], luma[index + width]) as u16;
                let gradient = ((gx + gy) / 2) as u8;

                gradient_samples += 1;
                if gradient < 10 {
                    flat_gradients += 1;
                } else if gradient > 38 {
                    strong_gradients += 1;
                } else {
                    mid_gradients += 1;
                }
            }
        }

        let mut low_variance_tiles = 0usize;
        let mut tile_count = 0usize;

        for tile_y in (0..height).step_by(4) {
            for tile_x in (0..width).step_by(4) {
                let x_end = (tile_x + 4).min(width);
                let y_end = (tile_y + 4).min(height);
                let sample_count = ((x_end - tile_x) * (y_end - tile_y)) as f32;
                if sample_count < 4.0 {
                    continue;
                }

                let mut sum = 0.0f32;
                let mut sum_sq = 0.0f32;
                for y in tile_y..y_end {
                    for x in tile_x..x_end {
                        let value = luma[y * width + x] as f32;
                        sum += value;
                        sum_sq += value * value;
                    }
                }

                let mean = sum / sample_count;
                let variance = (sum_sq / sample_count) - (mean * mean);
                if variance < 42.0 {
                    low_variance_tiles += 1;
                }
                tile_count += 1;
            }
        }

        let gradient_samples = gradient_samples.max(1) as f32;
        let tile_count = tile_count.max(1) as f32;
        frame_count += 1;
        flat_ratio_total += flat_gradients as f32 / gradient_samples;
        mid_ratio_total += mid_gradients as f32 / gradient_samples;
        strong_ratio_total += strong_gradients as f32 / gradient_samples;
        low_variance_ratio_total += low_variance_tiles as f32 / tile_count;
        palette_ratio_total += unique_bins as f32 / pixel_count.max(1) as f32;
    }

    if frame_count == 0 {
        return None;
    }

    let frame_count = frame_count as f32;
    let flat_ratio = flat_ratio_total / frame_count;
    let mid_ratio = mid_ratio_total / frame_count;
    let strong_ratio = strong_ratio_total / frame_count;
    let low_variance_ratio = low_variance_ratio_total / frame_count;
    let palette_ratio = palette_ratio_total / frame_count;
    let normalized_palette = ((palette_ratio - 0.06) / 0.22).clamp(0.0, 1.0);

    let animated_score = (low_variance_ratio * 0.55)
        + (flat_ratio * 0.25)
        + ((1.0 - normalized_palette) * 0.20)
        + (strong_ratio * 0.10);
    let live_score =
        ((1.0 - low_variance_ratio) * 0.40) + (mid_ratio * 0.30) + (normalized_palette * 0.30);
    let delta = animated_score - live_score;
    let confidence = ((delta.abs() - 0.12) / 0.38).clamp(0.0, 1.0);
    let kind = if delta > 0.12 {
        SourceContentKind::Animated
    } else if delta < -0.12 {
        SourceContentKind::LiveAction
    } else {
        SourceContentKind::Unknown
    };

    Some(SourceContentProbe { kind, confidence })
}

fn build_input_headers(extra_headers: &HashMap<String, String>) -> String {
    let mut header_str = format!("User-Agent: {}\r\n", REQUEST_USER_AGENT);
    for (key, value) in extra_headers {
        header_str.push_str(&format!("{}: {}\r\n", key, value));
    }
    header_str
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_source_descriptor, classify_rgb24_frames, classify_source_url, parse_hls_manifest,
        SourceContentKind, SourceKind, SourceMetadata, SourceTransport,
    };
    use std::collections::HashMap;

    #[test]
    fn classifies_remote_hls_inputs_for_shared_probe_and_relay_logic() {
        let classification = classify_source_url("https://example.com/live/master.m3u8");

        assert_eq!(classification.transport, SourceTransport::RemoteHttp);
        assert_eq!(classification.kind, SourceKind::Hls);
        assert!(classification.requires_local_hls_relay());
    }

    #[test]
    fn classifies_file_inputs_without_remote_transport() {
        let classification = classify_source_url("file:///tmp/playlist.m3u8");

        assert_eq!(classification.transport, SourceTransport::LocalFile);
        assert_eq!(classification.kind, SourceKind::Hls);
        assert!(!classification.requires_local_hls_relay());
    }

    #[test]
    fn parses_hls_manifest_into_source_metadata() {
        let body = r"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1280x720,FRAME-RATE=29.97
mid.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1920x1080,FRAME-RATE=59.94
hi.m3u8
";

        let metadata = parse_hls_manifest(body).unwrap();

        assert_eq!(metadata.width, Some(1920));
        assert_eq!(metadata.height, Some(1080));
        assert_eq!(metadata.source_resolution.as_deref(), Some("1920x1080"));
        assert_eq!(metadata.source_fps, Some(59.94));
        assert_eq!(metadata.content_kind, SourceContentKind::Unknown);
    }

    #[test]
    fn describes_relayed_source_with_runtime_relay_url_and_empty_runtime_headers() {
        let headers = HashMap::from([("Authorization".to_string(), "Bearer token".to_string())]);
        let metadata = Some(SourceMetadata {
            width: Some(1920),
            height: Some(1080),
            source_resolution: Some("1920x1080".to_string()),
            source_fps: Some(59.94),
            content_kind: SourceContentKind::Animated,
            content_kind_confidence: Some(0.92),
        });

        let descriptor = assemble_source_descriptor(
            "127.0.0.1",
            14002,
            "session-1",
            "https://example.com/live/master.m3u8",
            &headers,
            classify_source_url("https://example.com/live/master.m3u8"),
            metadata.clone(),
        );

        assert_eq!(
            descriptor.classification.transport,
            SourceTransport::RemoteHttp
        );
        assert_eq!(descriptor.classification.kind, SourceKind::Hls);
        assert_eq!(
            descriptor.runtime_url,
            "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8"
        );
        assert!(descriptor.runtime_headers.is_empty());
        assert_eq!(descriptor.session_headers, headers);
        assert_eq!(descriptor.metadata, metadata);
        assert!(descriptor.relay.is_some());
    }

    #[test]
    fn describes_non_relayed_source_with_original_runtime_url_and_headers() {
        let headers = HashMap::from([("X-Test".to_string(), "1".to_string())]);

        let descriptor = assemble_source_descriptor(
            "127.0.0.1",
            14002,
            "session-2",
            "https://example.com/video.mp4",
            &headers,
            classify_source_url("https://example.com/video.mp4"),
            None,
        );

        assert_eq!(
            descriptor.classification.transport,
            SourceTransport::RemoteHttp
        );
        assert_eq!(descriptor.classification.kind, SourceKind::Other);
        assert_eq!(descriptor.runtime_url, "https://example.com/video.mp4");
        assert_eq!(descriptor.runtime_headers, headers);
        assert_eq!(descriptor.session_headers, headers);
        assert!(descriptor.relay.is_none());
        assert!(descriptor.metadata.is_none());
    }

    #[test]
    fn classifies_flat_palette_frames_as_animated() {
        let width = 8usize;
        let height = 8usize;
        let mut frame = vec![0u8; width * height * 3];

        for y in 0..height {
            for x in 0..width {
                let index = (y * width + x) * 3;
                let (r, g, b) = if x < width / 2 {
                    (242, 80, 96)
                } else {
                    (42, 96, 224)
                };
                frame[index] = r;
                frame[index + 1] = g;
                frame[index + 2] = b;
            }
        }

        let probe = classify_rgb24_frames(&frame, width, height).expect("classification");
        assert_eq!(probe.kind, SourceContentKind::Animated);
        assert!(probe.confidence >= 0.0);
    }

    #[test]
    fn classifies_textured_frames_as_live_action() {
        let width = 8usize;
        let height = 8usize;
        let mut frame = vec![0u8; width * height * 3];

        for y in 0..height {
            for x in 0..width {
                let seed = ((x * 37 + y * 19) % 251) as u8;
                let index = (y * width + x) * 3;
                frame[index] = seed;
                frame[index + 1] = seed.wrapping_mul(3).wrapping_add(41);
                frame[index + 2] = seed.wrapping_mul(5).wrapping_add(17);
            }
        }

        let probe = classify_rgb24_frames(&frame, width, height).expect("classification");
        assert_eq!(probe.kind, SourceContentKind::LiveAction);
        assert!(probe.confidence >= 0.0);
    }
}
