use axum::body::Body;
use axum::http::StatusCode;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::classify_source_url;

const INPUT_PROXY_USER_AGENT: &str = "REFEREE-InputProxy";

static INPUT_PROXY_HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> =
    std::sync::LazyLock::new(|| {
        reqwest::Client::builder()
            .user_agent(INPUT_PROXY_USER_AGENT)
            .build()
            .expect("Failed to build input proxy HTTP client")
    });
static HLS_URI_ATTR_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r#"URI="([^"]+)""#).unwrap());

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayedSourceMetadata {
    pub manifest_content_type_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayedSource {
    pub original_url: String,
    pub relay_url: String,
    pub headers: HashMap<String, String>,
    pub metadata: Option<RelayedSourceMetadata>,
}

pub fn should_relay_input(source_url: &str) -> bool {
    classify_source_url(source_url).requires_local_hls_relay()
}

pub fn describe_relayed_source(
    relay_host: &str,
    relay_port: u16,
    session_id: &str,
    original_url: &str,
    headers: &HashMap<String, String>,
) -> Option<RelayedSource> {
    should_relay_input(original_url).then(|| RelayedSource {
        original_url: original_url.to_string(),
        relay_url: build_relay_url(relay_host, relay_port, session_id, original_url),
        headers: headers.clone(),
        metadata: None,
    })
}

pub fn build_relay_url(
    relay_host: &str,
    relay_port: u16,
    session_id: &str,
    target_url: &str,
) -> String {
    let mut proxy_url = reqwest::Url::parse(&format!(
        "http://{}:{}/v1/input/{}",
        relay_host, relay_port, session_id
    ))
    .expect("input proxy base URL should be valid");
    proxy_url.query_pairs_mut().append_pair("url", target_url);
    proxy_url.to_string()
}

pub async fn relay_request(
    relay_host: &str,
    relay_port: u16,
    session_id: &str,
    source_headers: &HashMap<String, String>,
    target_url: &str,
) -> Result<axum::response::Response, StatusCode> {
    let target_url = reqwest::Url::parse(target_url).map_err(|_| StatusCode::BAD_REQUEST)?;
    if !matches!(target_url.scheme(), "http" | "https") {
        return Err(StatusCode::BAD_REQUEST);
    }

    // SSRF protection: block loopback and link-local targets.
    // RFC-1918 private ranges are allowed so LAN stream sources remain reachable.
    if super::is_ssrf_target(target_url.as_str()).await {
        tracing::warn!(
            "[Input Proxy]: Blocked SSRF attempt for session {}: {}",
            session_id,
            target_url
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut request = INPUT_PROXY_HTTP_CLIENT.get(target_url.clone());
    for (key, value) in source_headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let upstream = request.send().await.map_err(|error| {
        tracing::warn!(
            "[Input Proxy]: Failed to fetch upstream URL for session {}: {}",
            session_id,
            error
        );
        StatusCode::BAD_GATEWAY
    })?;

    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);

    if is_hls_manifest_response(content_type.as_deref(), &target_url) {
        let body = upstream.text().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
        let rewritten =
            rewrite_hls_manifest(relay_host, relay_port, session_id, &target_url, &body);
        let manifest_content_type = content_type
            .as_deref()
            .unwrap_or("application/vnd.apple.mpegurl");

        return Ok(axum::response::Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, manifest_content_type)
            .header("Access-Control-Allow-Origin", "*")
            .body(Body::from(rewritten))
            .unwrap());
    }

    let mut response = axum::response::Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*");

    if let Some(content_type) = content_type.as_deref() {
        response = response.header(axum::http::header::CONTENT_TYPE, content_type);
    }

    Ok(response
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap())
}

fn is_hls_manifest_response(content_type: Option<&str>, target_url: &reqwest::Url) -> bool {
    let path = target_url.path().to_ascii_lowercase();
    let path_looks_like_manifest = path.ends_with(".m3u8") || path.ends_with(".m3u");
    let content_type_looks_like_manifest = content_type
        .map(|value| value.to_ascii_lowercase())
        .map(|value| {
            value.contains("application/vnd.apple.mpegurl")
                || value.contains("application/x-mpegurl")
                || value.contains("audio/mpegurl")
        })
        .unwrap_or(false);

    path_looks_like_manifest || content_type_looks_like_manifest
}

pub fn is_hls_manifest_url(target_url: &reqwest::Url) -> bool {
    is_hls_manifest_response(None, target_url)
}

pub fn is_hls_manifest_content_type(content_type: Option<&str>) -> bool {
    is_hls_manifest_response(
        content_type,
        &reqwest::Url::parse("https://example.invalid/").unwrap(),
    )
}

pub fn resolve_relay_target(base_url: &reqwest::Url, target: &str) -> Option<reqwest::Url> {
    let resolved = reqwest::Url::parse(target)
        .or_else(|_| base_url.join(target))
        .ok()?;

    if !matches!(resolved.scheme(), "http" | "https") {
        return None;
    }

    Some(resolved)
}

fn resolve_proxy_target(
    base_url: &reqwest::Url,
    target: &str,
    mut build_proxy_url: impl FnMut(&reqwest::Url) -> Option<String>,
) -> Option<String> {
    let resolved = resolve_relay_target(base_url, target)?;
    build_proxy_url(&resolved)
}

pub fn rewrite_hls_manifest_with_target_resolver(
    base_url: &reqwest::Url,
    body: &str,
    mut build_proxy_url: impl FnMut(&reqwest::Url) -> Option<String>,
) -> String {
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let had_trailing_newline = body.ends_with('\n');

    let lines: Vec<&str> = body.lines().collect();

    let best_bw = best_video_variant_bandwidth(&lines);

    let mut output: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    let mut best_bw_emitted = false;

    while i < lines.len() {
        let line = lines[i];

        if let Some(best) = best_bw {
            if line.starts_with("#EXT-X-STREAM-INF:") && line.contains("RESOLUTION=") {
                let this_bw = parse_stream_inf_bandwidth(line);
                let is_best = this_bw == Some(best);
                if !is_best || best_bw_emitted {
                    i += 1;
                    while i < lines.len() && lines[i].trim().is_empty() {
                        i += 1;
                    }
                    if i < lines.len() && !lines[i].starts_with('#') {
                        i += 1;
                    }
                    continue;
                }
                best_bw_emitted = true;
            }
        }

        output.push(rewrite_hls_manifest_line(
            base_url,
            line,
            &mut build_proxy_url,
        ));
        i += 1;
    }

    let mut rewritten = output.join(newline);
    if had_trailing_newline {
        rewritten.push_str(newline);
    }
    rewritten
}

pub fn rewrite_hls_manifest(
    relay_host: &str,
    relay_port: u16,
    session_id: &str,
    base_url: &reqwest::Url,
    body: &str,
) -> String {
    rewrite_hls_manifest_with_target_resolver(base_url, body, |resolved| {
        Some(build_relay_url(
            relay_host,
            relay_port,
            session_id,
            resolved.as_str(),
        ))
    })
}

/// Extracts the `BANDWIDTH` attribute value from a `#EXT-X-STREAM-INF` line.
fn parse_stream_inf_bandwidth(line: &str) -> Option<u64> {
    let attrs = line.strip_prefix("#EXT-X-STREAM-INF:")?;
    // BANDWIDTH is always a plain integer — splitting on comma is safe here
    // even when CODECS= contains commas inside quotes.
    attrs.split(',').find_map(|attr| {
        attr.trim()
            .strip_prefix("BANDWIDTH=")
            .and_then(|v| v.parse::<u64>().ok())
    })
}

/// Returns the highest `BANDWIDTH` value among `#EXT-X-STREAM-INF` entries
/// that carry a `RESOLUTION=` tag (video variants).  Returns `None` when
/// the manifest has fewer than two video variants — nothing to filter.
fn best_video_variant_bandwidth(lines: &[&str]) -> Option<u64> {
    let bandwidths: Vec<u64> = lines
        .iter()
        .filter(|l| l.starts_with("#EXT-X-STREAM-INF:") && l.contains("RESOLUTION="))
        .filter_map(|l| parse_stream_inf_bandwidth(l))
        .collect();

    if bandwidths.len() <= 1 {
        return None;
    }
    bandwidths.into_iter().max()
}

fn rewrite_hls_manifest_line(
    base_url: &reqwest::Url,
    line: &str,
    build_proxy_url: &mut impl FnMut(&reqwest::Url) -> Option<String>,
) -> String {
    if line.trim().is_empty() {
        return line.to_string();
    }

    if line.starts_with('#') {
        return HLS_URI_ATTR_RE
            .replace_all(line, |caps: &regex::Captures| {
                resolve_proxy_target(base_url, &caps[1], |resolved| build_proxy_url(resolved))
                    .map(|url| format!(r#"URI="{}""#, url))
                    .unwrap_or_else(|| caps[0].to_string())
            })
            .into_owned();
    }

    resolve_proxy_target(base_url, line.trim(), |resolved| build_proxy_url(resolved))
        .unwrap_or_else(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        build_relay_url, describe_relayed_source, resolve_relay_target, rewrite_hls_manifest,
        rewrite_hls_manifest_with_target_resolver, should_relay_input,
    };
    use std::collections::HashMap;

    const RELAY_HOST: &str = "127.0.0.1";
    const RELAY_PORT: u16 = 14002;

    #[test]
    fn rewrites_relative_manifest_entries_to_local_proxy_urls() {
        let base_url = reqwest::Url::parse("https://example.com/live/master.m3u8").unwrap();
        let body = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.key\"\nsegment0.ts\n";
        let rewritten = rewrite_hls_manifest(RELAY_HOST, RELAY_PORT, "session-1", &base_url, body);

        assert!(rewritten.contains(
            "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fkey.key"
        ));
        assert!(rewritten.contains(
            "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fsegment0.ts"
        ));
    }

    #[test]
    fn relay_url_encodes_target_query_parameters() {
        let proxied = build_relay_url(
            RELAY_HOST,
            RELAY_PORT,
            "abc123",
            "https://example.com/playlist.m3u8?token=a+b&sig=1/2",
        );

        assert_eq!(
            proxied,
            "http://127.0.0.1:14002/v1/input/abc123?url=https%3A%2F%2Fexample.com%2Fplaylist.m3u8%3Ftoken%3Da%2Bb%26sig%3D1%2F2"
        );
    }

    #[test]
    fn relays_remote_hls_inputs() {
        assert!(should_relay_input("https://example.com/live/master.m3u8"));
        assert!(should_relay_input(
            "https://example.com/watch?playlist=https://cdn.example.com/live/channel.m3u8"
        ));
    }

    #[test]
    fn filters_master_playlist_to_highest_bandwidth_video_variant() {
        let base_url = reqwest::Url::parse("https://example.com/live/master.m3u8").unwrap();
        let body = concat!(
            "#EXTM3U\n",
            "#EXT-X-STREAM-INF:BANDWIDTH=493000,CODECS=\"mp4a.40.2,avc1.66.30\",RESOLUTION=224x100,FRAME-RATE=24\n",
            "low.m3u8\n",
            "#EXT-X-STREAM-INF:BANDWIDTH=1727000,CODECS=\"mp4a.40.2,avc1.100.40\",RESOLUTION=1680x750,FRAME-RATE=24\n",
            "high.m3u8\n",
            "#EXT-X-STREAM-INF:BANDWIDTH=68000,CODECS=\"mp4a.40.2\"\n",
            "audio.m3u8\n",
        );
        let rewritten = rewrite_hls_manifest(RELAY_HOST, RELAY_PORT, "session-1", &base_url, body);
        // The low-quality video variant must be gone.
        assert!(
            !rewritten.contains("224x100"),
            "low-res INF should be stripped"
        );
        assert!(
            !rewritten.contains("low.m3u8"),
            "low-res URI should be stripped"
        );
        // The high-quality video variant must be kept (URL rewritten through relay).
        assert!(
            rewritten.contains("1680x750"),
            "high-res INF should be present"
        );
        assert!(
            rewritten.contains("session-1") && rewritten.contains("high.m3u8"),
            "high-res URI should be rewritten through relay"
        );
        // Audio-only variant (no RESOLUTION=) must be kept.
        assert!(
            rewritten.contains("audio.m3u8"),
            "audio-only variant should be preserved"
        );
    }

    #[test]
    fn does_not_filter_single_video_variant_playlist() {
        let base_url = reqwest::Url::parse("https://example.com/live/master.m3u8").unwrap();
        let body = concat!(
            "#EXTM3U\n",
            "#EXT-X-STREAM-INF:BANDWIDTH=1727000,RESOLUTION=1680x750\n",
            "only.m3u8\n",
        );
        let rewritten = rewrite_hls_manifest(RELAY_HOST, RELAY_PORT, "session-1", &base_url, body);
        assert!(rewritten.contains("1680x750"));
        assert!(rewritten.contains("only.m3u8") || rewritten.contains("session-1"));
    }

    #[test]
    fn does_not_relay_non_hls_or_non_remote_inputs() {
        assert!(!should_relay_input("https://example.com/video.mp4"));
        assert!(!should_relay_input("file:///tmp/playlist.m3u8"));
        assert!(!should_relay_input("C:\\streams\\playlist.m3u8"));
    }

    #[test]
    fn describes_relayed_source_for_future_source_planning() {
        let headers = HashMap::from([("Authorization".to_string(), "Bearer token".to_string())]);
        let descriptor = describe_relayed_source(
            RELAY_HOST,
            RELAY_PORT,
            "session-42",
            "https://example.com/live/master.m3u8",
            &headers,
        )
        .unwrap();

        assert_eq!(
            descriptor.original_url,
            "https://example.com/live/master.m3u8"
        );
        assert_eq!(
            descriptor.relay_url,
            "http://127.0.0.1:14002/v1/input/session-42?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8"
        );
        assert_eq!(descriptor.headers, headers);
        assert!(descriptor.metadata.is_none());
    }

    #[test]
    fn resolves_relative_targets_against_manifest_base_url() {
        let base_url = reqwest::Url::parse("https://example.com/live/master.m3u8").unwrap();
        let resolved = resolve_relay_target(&base_url, "segment0.ts").unwrap();

        assert_eq!(resolved.as_str(), "https://example.com/live/segment0.ts");
    }

    #[test]
    fn rewrites_manifest_with_custom_target_resolver() {
        let base_url = reqwest::Url::parse("https://example.com/live/master.m3u8").unwrap();
        let body = "#EXTM3U\nvariant.m3u8\n";
        let rewritten = rewrite_hls_manifest_with_target_resolver(&base_url, body, |resolved| {
            let mut proxy_url =
                reqwest::Url::parse("http://localhost:14002/v1/tmp/session-1/proxy").unwrap();
            proxy_url
                .query_pairs_mut()
                .append_pair("url", resolved.as_str());
            Some(proxy_url.to_string())
        });

        assert!(rewritten.contains(
            "http://localhost:14002/v1/tmp/session-1/proxy?url=https%3A%2F%2Fexample.com%2Flive%2Fvariant.m3u8"
        ));
    }
}
