//! Optional routing of stream sources through an EasyProxy server
//! (<https://github.com/realbestia1/EasyProxy>).
//!
//! When enabled, a locally processed stream's source URL first goes to
//! EasyProxy's extractor, which finds the video on the pages of sites it
//! supports, and the result is rewritten onto EasyProxy's proxy endpoint.
//! Client-supplied headers travel as `h_<Name>` query parameters, which
//! EasyProxy sends upstream and carries into the playlist and segment URLs it
//! rewrites. REFEREE's input proxy stays in front of the encoder, so variant
//! pinning and stall timeouts still apply.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use super::{classify_source_url, is_literal_ssrf_target};

/// Where EasyProxy listens when started with its defaults.
pub const DEFAULT_EASY_PROXY_URL: &str = "http://127.0.0.1:7860";
/// The port in [`DEFAULT_EASY_PROXY_URL`].
pub const DEFAULT_EASY_PROXY_PORT: u16 = 7860;
/// EasyProxy reads its `API_PASSWORD` from this header or from an
/// `api_password` query parameter. The header keeps it out of URLs and logs.
const API_PASSWORD_HEADER: &str = "x-api-password";
const HLS_ENDPOINT: &str = "proxy/manifest.m3u8";
const STREAM_ENDPOINT: &str = "proxy/stream";
const HEALTH_ENDPOINT: &str = "health";
const EXTRACTOR_ENDPOINT: &str = "extractor/video";
/// The `extractor_key` EasyProxy reports when no site extractor handles a URL.
const GENERIC_EXTRACTOR: &str = "generic";
/// EasyProxy's slowest extractors allow themselves 180 s, so its own timeout
/// error arrives before this one.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(190);
/// Paths EasyProxy writes into rewritten playlists. The input proxy reaches
/// nothing else on the server, which keeps EasyProxy's admin and recording
/// APIs out of reach of whoever holds a session ID.
const MEDIA_PATH_PREFIXES: &[&str] = &["/proxy/", "/segment/", "/decrypt/"];
const KEY_PATH: &str = "/key";
/// Names the outbound proxy EasyProxy itself uses, such as its WARP endpoint
/// on `socks5://127.0.0.1:1080`, so loopback values there are expected.
const OUTBOUND_PROXY_PARAM: &str = "proxy";
const CHECK_TIMEOUT: Duration = Duration::from_secs(3);
const LOG_REDACTED: &str = "<redacted>";

/// Times requests out after [`CHECK_TIMEOUT`] unless one sets its own.
static HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(CHECK_TIMEOUT)
        .build()
        .expect("Failed to build EasyProxy HTTP client")
});

/// A configured EasyProxy server that stream sources are routed through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EasyProxyRoute {
    base_url: reqwest::Url,
    api_password: Option<String>,
}

/// A source URL rewritten onto EasyProxy, with the headers to send to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedSource {
    pub url: String,
    pub headers: HashMap<String, String>,
}

/// A source after EasyProxy's extractor has seen it, routed through EasyProxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSource {
    /// What probes and the encoder read.
    pub routed: RoutedSource,
    /// Headers the video's host expects, for targets the input proxy routes
    /// itself: the extractor's for a video it found, else the client's.
    pub source_headers: HashMap<String, String>,
    /// The video a site extractor found, or `None` when EasyProxy has no
    /// extractor for the source and it is routed as it is.
    pub video_url: Option<String>,
}

/// What `/extractor/video` answers, in MediaFlow-Proxy's format.
#[derive(Debug, Deserialize)]
struct Extraction {
    destination_url: String,
    #[serde(default)]
    request_headers: HashMap<String, String>,
    mediaflow_proxy_url: String,
    #[serde(default)]
    query_params: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EasyProxyStatus {
    /// Version reported by `/health`. Servers without that route report none.
    pub version: Option<String>,
}

impl EasyProxyRoute {
    /// A route to the server at `url` (the local default when blank).
    pub fn new(url: Option<&str>, api_password: Option<&str>) -> Result<Self, String> {
        let raw = url
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .unwrap_or(DEFAULT_EASY_PROXY_URL);
        let base_url = reqwest::Url::parse(raw)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
            .ok_or_else(|| {
                format!("EasyProxy URL \"{raw}\" must be an http:// or https:// address.")
            })?;
        // EasyProxy writes its rewritten URLs from scheme and host alone, so it
        // cannot sit under a path prefix. Credentials would leak into logged URLs.
        if base_url.path() != "/"
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
        {
            return Err(format!(
                "EasyProxy URL \"{raw}\" should be the server address only, like {DEFAULT_EASY_PROXY_URL}."
            ));
        }

        Ok(Self {
            base_url,
            api_password: api_password
                .map(str::trim)
                .filter(|password| !password.is_empty())
                .map(String::from),
        })
    }

    /// The server's origin, for logs and messages. Never carries credentials.
    pub fn origin(&self) -> String {
        self.base_url.origin().ascii_serialization()
    }

    /// Whether `url` points at this EasyProxy server.
    pub fn serves(&self, url: &reqwest::Url) -> bool {
        url.origin() == self.base_url.origin()
    }

    /// Headers for a request to EasyProxy itself. Only ever sent to
    /// [`Self::serves`] URLs, so the password never reaches another host.
    pub fn request_headers(&self) -> HashMap<String, String> {
        self.api_password
            .iter()
            .map(|password| (API_PASSWORD_HEADER.to_string(), password.clone()))
            .collect()
    }

    /// Rewrites `source_url` onto EasyProxy, moving `headers` into `h_`
    /// parameters for EasyProxy to send upstream. A URL already on this
    /// server is kept as it is.
    pub fn route(&self, source_url: &str, headers: &HashMap<String, String>) -> RoutedSource {
        let already_routed = reqwest::Url::parse(source_url).is_ok_and(|url| self.serves(&url));
        if already_routed {
            return RoutedSource {
                url: source_url.to_string(),
                headers: self.request_headers(),
            };
        }

        // EasyProxy rewrites playlists served from its manifest endpoint and
        // passes anything else through. The `.m3u8` path also keeps REFEREE's
        // input proxy in front of the encoder for HLS sources.
        let endpoint = if classify_source_url(source_url).is_hls_like() {
            HLS_ENDPOINT
        } else {
            STREAM_ENDPOINT
        };
        let mut url = self.endpoint(endpoint);
        url.query_pairs_mut()
            .append_pair("url", source_url)
            .extend_pairs(header_params(headers));

        RoutedSource {
            url: url.into(),
            headers: self.request_headers(),
        }
    }

    /// Asks EasyProxy's extractor for the video behind `source_url`, such as
    /// the stream on a supported site's page, and routes that video. A source
    /// no site extractor handles, such as a direct stream URL, is routed as it
    /// is with [`Self::route`]: EasyProxy's generic extractor would only
    /// percent-decode it a second time.
    pub async fn extract(
        &self,
        source_url: &str,
        headers: &HashMap<String, String>,
    ) -> Result<ExtractedSource, String> {
        let as_is = || ExtractedSource {
            routed: self.route(source_url, headers),
            source_headers: headers.clone(),
            video_url: None,
        };
        if reqwest::Url::parse(source_url).is_ok_and(|url| self.serves(&url)) {
            return Ok(as_is());
        }

        let mut url = self.endpoint(EXTRACTOR_ENDPOINT);
        url.query_pairs_mut()
            .append_pair("url", source_url)
            .extend_pairs(header_params(headers));
        let mut request = HTTP_CLIENT.get(url).timeout(EXTRACT_TIMEOUT);
        for (name, value) in self.request_headers() {
            request = request.header(name, value);
        }

        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                extraction_failed(&format!(
                    "no answer within {} s.",
                    EXTRACT_TIMEOUT.as_secs()
                ))
            } else {
                format!(
                    "Could not reach EasyProxy at {}: {}",
                    self.origin(),
                    error.without_url()
                )
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            // A failed extraction answers `{"error": "..."}`.
            let detail = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| body.get("error")?.as_str().map(String::from))
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            return Err(extraction_failed(&detail));
        }
        let extraction: Extraction = response
            .json()
            .await
            .map_err(|_| extraction_failed("its answer was not an extraction result."))?;

        if extraction
            .query_params
            .get("extractor_key")
            .is_some_and(|key| key == GENERIC_EXTRACTOR)
        {
            return Ok(as_is());
        }

        // Only the path is taken: EasyProxy writes its own public address
        // into this URL, which can differ from the one REFEREE reaches it on.
        let endpoint = reqwest::Url::parse(&extraction.mediaflow_proxy_url)
            .map_err(|_| extraction_failed("it named no proxy endpoint for the video."))?;
        let mut extractor_params: Vec<_> = extraction.query_params.iter().collect();
        extractor_params.sort();
        let mut url = self.endpoint(endpoint.path());
        url.query_pairs_mut()
            .append_pair("d", &extraction.destination_url)
            .extend_pairs(header_params(&extraction.request_headers))
            .extend_pairs(extractor_params)
            // Lets EasyProxy extract the source again when the video's link expires.
            .append_pair("orig_url", source_url);
        if !self.allows_relay_target(&url) {
            return Err(extraction_failed(
                "the video it found is at an address REFEREE will not fetch.",
            ));
        }

        Ok(ExtractedSource {
            routed: RoutedSource {
                url: url.into(),
                headers: self.request_headers(),
            },
            source_headers: extraction.request_headers,
            video_url: Some(extraction.destination_url),
        })
    }

    /// Whether the input proxy may fetch `url`, a URL on this server: one of
    /// EasyProxy's media paths, where every http(s) URL in the query (`url`,
    /// `d`, `key_url`, `orig_url`, ...) is not loopback or link-local. Targets
    /// are checked without a DNS lookup because EasyProxy, not REFEREE,
    /// connects to them, and a lookup here would send the hostname outside
    /// the network path EasyProxy provides.
    pub fn allows_relay_target(&self, url: &reqwest::Url) -> bool {
        let path = url.path();
        let media_path = path == KEY_PATH
            || MEDIA_PATH_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix));

        media_path
            && !url.query_pairs().any(|(name, value)| {
                name != OUTBOUND_PROXY_PARAM
                    && is_http_url(&value)
                    && is_literal_ssrf_target(&value)
            })
    }

    /// Confirms the server is EasyProxy and accepts the API password, so a
    /// misconfiguration fails stream start with a clear message.
    pub async fn check(&self) -> Result<EasyProxyStatus, String> {
        let origin = self.origin();
        let unreachable = |error: reqwest::Error| {
            if error.is_timeout() {
                format!(
                    "EasyProxy at {origin} did not respond within {} s.",
                    CHECK_TIMEOUT.as_secs()
                )
            } else if error.is_connect() {
                format!("Could not connect to EasyProxy at {origin}. Check that it is running.")
            } else {
                format!(
                    "Could not reach EasyProxy at {origin}: {}",
                    error.without_url()
                )
            }
        };

        // `/health` reports the version but skips the password check.
        let version = match HTTP_CLIENT.get(self.endpoint(HEALTH_ENDPOINT)).send().await {
            Ok(response) if response.status().is_success() => response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|health| health.get("version")?.as_str().map(String::from)),
            Ok(_) => None,
            Err(error) => return Err(unreachable(error)),
        };

        // Without a `url` the proxy endpoint answers 400 once the password
        // check passes, and 401 when it fails.
        let mut request = HTTP_CLIENT.get(self.endpoint(HLS_ENDPOINT));
        for (name, value) in self.request_headers() {
            request = request.header(name, value);
        }
        match request.send().await.map_err(unreachable)?.status() {
            reqwest::StatusCode::BAD_REQUEST => Ok(EasyProxyStatus { version }),
            reqwest::StatusCode::UNAUTHORIZED if self.api_password.is_some() => {
                Err("EasyProxy rejected the API password.".to_string())
            }
            reqwest::StatusCode::UNAUTHORIZED => {
                Err("EasyProxy requires an API password.".to_string())
            }
            status => Err(format!(
                "The server at {origin} did not respond like EasyProxy (HTTP {}).",
                status.as_u16()
            )),
        }
    }

    fn endpoint(&self, path: &str) -> reqwest::Url {
        self.base_url
            .join(path)
            .expect("EasyProxy endpoints are relative paths")
    }
}

/// `h_<Name>` parameters for `headers`, sorted so URLs are stable.
fn header_params(headers: &HashMap<String, String>) -> Vec<(String, &str)> {
    let mut params: Vec<_> = headers
        .iter()
        .map(|(name, value)| (format!("h_{name}"), value.as_str()))
        .collect();
    params.sort();
    params
}

fn extraction_failed(detail: &str) -> String {
    format!("EasyProxy could not find a video at the source URL: {detail}")
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Masks `h_*` header values and `api_password` in a URL's query, including
/// inside nested URLs such as a `url=` parameter, before the URL is logged.
/// Client headers can carry credentials, and routing moves them into the URL.
pub fn redact_url_for_log(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.query().is_none() {
        return url.to_string();
    }

    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(name, value)| {
            let value = if name.starts_with("h_") || name == "api_password" {
                LOG_REDACTED.to_string()
            } else if is_http_url(&value) {
                redact_url_for_log(&value)
            } else {
                value.into_owned()
            };
            (name.into_owned(), value)
        })
        .collect();
    parsed.query_pairs_mut().clear().extend_pairs(pairs);
    parsed.into()
}

#[cfg(test)]
mod tests {
    use super::{redact_url_for_log, EasyProxyRoute, DEFAULT_EASY_PROXY_URL};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn route(url: &str, password: Option<&str>) -> EasyProxyRoute {
        EasyProxyRoute::new(Some(url), password).unwrap()
    }

    fn url(raw: &str) -> reqwest::Url {
        reqwest::Url::parse(raw).unwrap()
    }

    type SeenQueries = Arc<Mutex<Vec<HashMap<String, String>>>>;

    /// Serves `/extractor/video` the way EasyProxy does for a site page
    /// (`/watch`), a page without a video (`/missing`), a page whose video is
    /// on loopback (`/local`), and anything else (the generic extractor).
    /// Returns its address and the queries it was sent.
    async fn spawn_mock_extractor(password: &'static str) -> (String, SeenQueries) {
        use axum::extract::Query;
        use axum::http::StatusCode;
        use serde_json::json;

        let seen = SeenQueries::default();
        let recorder = seen.clone();
        let extractor =
            move |headers: axum::http::HeaderMap, Query(query): Query<HashMap<String, String>>| {
                let recorder = recorder.clone();
                async move {
                    let authorized = headers
                        .get("x-api-password")
                        .is_some_and(|value| value == password);
                    if !authorized {
                        return (StatusCode::UNAUTHORIZED, axum::Json(json!({})));
                    }
                    recorder.lock().unwrap().push(query.clone());
                    let source = query.get("url").cloned().unwrap_or_default();
                    let found = |video: &str| {
                        json!({
                            "destination_url": video,
                            "request_headers": {
                                "Referer": "https://site.example/",
                                "User-Agent": "Mozilla/5.0",
                            },
                            "mediaflow_endpoint": "hls_proxy",
                            "mediaflow_proxy_url": "https://public.example/proxy/hls/manifest.m3u8",
                            "query_params": { "extractor_key": "site", "stream_key": "abc-1" },
                        })
                    };
                    match source.as_str() {
                        "https://site.example/watch/1" => (
                            StatusCode::OK,
                            axum::Json(found("https://cdn.example.com/live/index.m3u8?t=1")),
                        ),
                        "https://site.example/local" => (
                            StatusCode::OK,
                            axum::Json(found("http://127.0.0.1:22/index.m3u8")),
                        ),
                        "https://site.example/missing" => (
                            StatusCode::NOT_FOUND,
                            axum::Json(json!({ "error": "Video not found", "status": "error" })),
                        ),
                        _ => (
                            StatusCode::OK,
                            axum::Json(json!({
                                "destination_url": source,
                                "request_headers": { "user-agent": "Mozilla/5.0" },
                                "mediaflow_endpoint": "hls_proxy",
                                "mediaflow_proxy_url": "http://127.0.0.1/proxy/hls/manifest.m3u8",
                                "query_params": { "extractor_key": "generic" },
                            })),
                        ),
                    }
                }
            };
        let app = axum::Router::new().route("/extractor/video", axum::routing::get(extractor));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), seen)
    }

    #[tokio::test]
    async fn extraction_routes_the_video_a_site_extractor_found() {
        let (base_url, seen) = spawn_mock_extractor("ep").await;
        let route = route(&base_url, Some("ep"));
        let client_headers =
            HashMap::from([("Referer".to_string(), "https://app.example/".to_string())]);

        let extracted = route
            .extract("https://site.example/watch/1", &client_headers)
            .await
            .unwrap();

        // The extractor saw the page URL and the client's headers as `h_` params.
        let query = seen.lock().unwrap().pop().unwrap();
        assert_eq!(query["url"], "https://site.example/watch/1");
        assert_eq!(query["h_Referer"], "https://app.example/");

        // The video is played from REFEREE's address for EasyProxy, not the
        // public one EasyProxy reported, with the extractor's headers.
        assert_eq!(
            extracted.routed.url,
            format!(
                "{base_url}/proxy/hls/manifest.m3u8?d=https%3A%2F%2Fcdn.example.com%2Flive%2Findex.m3u8%3Ft%3D1\
                 &h_Referer=https%3A%2F%2Fsite.example%2F&h_User-Agent=Mozilla%2F5.0\
                 &extractor_key=site&stream_key=abc-1\
                 &orig_url=https%3A%2F%2Fsite.example%2Fwatch%2F1"
            )
        );
        assert_eq!(
            extracted.routed.headers,
            HashMap::from([("x-api-password".to_string(), "ep".to_string())])
        );
        assert_eq!(
            extracted.source_headers,
            HashMap::from([
                ("Referer".to_string(), "https://site.example/".to_string()),
                ("User-Agent".to_string(), "Mozilla/5.0".to_string()),
            ])
        );
        assert_eq!(
            extracted.video_url.as_deref(),
            Some("https://cdn.example.com/live/index.m3u8?t=1")
        );
        assert!(route.allows_relay_target(&url(&extracted.routed.url)));
        assert!(
            crate::source::classify_source_url(&extracted.routed.url).requires_local_hls_relay()
        );
    }

    #[tokio::test]
    async fn sources_without_a_site_extractor_are_routed_as_they_are() {
        let (base_url, seen) = spawn_mock_extractor("ep").await;
        let route = route(&base_url, Some("ep"));
        let headers = HashMap::from([("Cookie".to_string(), "a=1".to_string())]);
        // EasyProxy would decode `%2B` a second time; the route keeps it.
        let source = "https://cdn.example.com/live/master.m3u8?sig=a%2Bb";

        let extracted = route.extract(source, &headers).await.unwrap();

        assert_eq!(extracted.routed, route.route(source, &headers));
        assert_eq!(extracted.source_headers, headers);
        assert_eq!(extracted.video_url, None);
        assert_eq!(seen.lock().unwrap().len(), 1);

        // Already on EasyProxy: kept, and the extractor is not asked.
        let routed = extracted.routed.url;
        let again = route.extract(&routed, &headers).await.unwrap();
        assert_eq!(again.routed.url, routed);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn extraction_failures_carry_easy_proxy_s_reason() {
        let (base_url, _) = spawn_mock_extractor("ep").await;
        let route = route(&base_url, Some("ep"));
        let no_headers = HashMap::new();

        let message = route
            .extract("https://site.example/missing", &no_headers)
            .await
            .unwrap_err();
        assert!(message.ends_with(": Video not found"), "{message}");

        // A video on loopback would let the source reach REFEREE's machine.
        let message = route
            .extract("https://site.example/local", &no_headers)
            .await
            .unwrap_err();
        assert!(message.contains("will not fetch"), "{message}");

        let message = super::EasyProxyRoute::new(Some(&base_url), Some("nope"))
            .unwrap()
            .extract("https://site.example/watch/1", &no_headers)
            .await
            .unwrap_err();
        assert!(message.ends_with("HTTP 401"), "{message}");
    }

    #[test]
    fn route_defaults_to_the_local_server() {
        for blank in [None, Some(""), Some("   ")] {
            let route = EasyProxyRoute::new(blank, Some(" ")).unwrap();
            assert_eq!(route.origin(), DEFAULT_EASY_PROXY_URL);
            assert!(route.request_headers().is_empty());
        }
    }

    #[test]
    fn route_rejects_non_http_urls() {
        for invalid in [
            "127.0.0.1:7860",
            "ftp://127.0.0.1",
            "not a url",
            "file:///tmp",
            "http://127.0.0.1:7860/admin",
            "http://127.0.0.1:7860/?api_password=ep",
            "http://user:secret@127.0.0.1:7860",
        ] {
            assert!(
                EasyProxyRoute::new(Some(invalid), None).is_err(),
                "{invalid} should be rejected"
            );
        }
    }

    #[test]
    fn routes_hls_sources_through_the_manifest_endpoint_with_headers_as_params() {
        let headers = HashMap::from([
            ("Referer".to_string(), "https://example.com/".to_string()),
            ("User-Agent".to_string(), "VLC".to_string()),
        ]);
        let routed = route("http://127.0.0.1:7860", Some("ep")).route(
            "https://cdn.example.com/live/master.m3u8?token=a&b=1",
            &headers,
        );

        assert_eq!(
            routed.url,
            "http://127.0.0.1:7860/proxy/manifest.m3u8?url=https%3A%2F%2Fcdn.example.com%2Flive%2Fmaster.m3u8%3Ftoken%3Da%26b%3D1&h_Referer=https%3A%2F%2Fexample.com%2F&h_User-Agent=VLC"
        );
        // The password travels as a header, never in the URL.
        assert!(!routed.url.contains("api_password"));
        assert_eq!(
            routed.headers,
            HashMap::from([("x-api-password".to_string(), "ep".to_string())])
        );
        assert!(crate::source::classify_source_url(&routed.url).requires_local_hls_relay());
    }

    #[test]
    fn routes_other_sources_through_the_stream_endpoint() {
        let routed = route("http://media-box:7860/", None)
            .route("https://cdn.example.com/video.mp4", &HashMap::new());

        assert_eq!(
            routed.url,
            "http://media-box:7860/proxy/stream?url=https%3A%2F%2Fcdn.example.com%2Fvideo.mp4"
        );
        assert!(routed.headers.is_empty());
        assert!(!crate::source::classify_source_url(&routed.url).is_hls_like());
    }

    #[test]
    fn leaves_already_routed_urls_alone() {
        let route = route("http://media-box:7860/", None);
        let routed = route.route("https://cdn.example.com/a.m3u8", &HashMap::new());
        assert!(routed
            .url
            .starts_with("http://media-box:7860/proxy/manifest.m3u8?url="));

        let again = route.route(&routed.url, &HashMap::new());
        assert_eq!(again.url, routed.url);
    }

    #[test]
    fn relay_targets_are_limited_to_media_paths_on_the_server() {
        let route = route("http://127.0.0.1:7860", Some("ep"));

        assert!(route.serves(&url("http://127.0.0.1:7860/proxy/hls/segment.ts?d=x")));
        assert!(!route.serves(&url("http://127.0.0.1:7861/proxy/hls/segment.ts")));
        assert!(!route.serves(&url("https://127.0.0.1:7860/proxy/hls/segment.ts")));

        for allowed in [
            "http://127.0.0.1:7860/proxy/hls/manifest.m3u8?url=https%3A%2F%2Fcdn.example.com%2Fa.m3u8",
            "http://127.0.0.1:7860/proxy/hls/segment.ts?d=https%3A%2F%2Fcdn.example.com%2F1.ts",
            "http://127.0.0.1:7860/key?key_url=https%3A%2F%2Fcdn.example.com%2Fkey.bin",
            "http://127.0.0.1:7860/proxy/hls/segment.ts?d=https%3A%2F%2Fcdn.example.com%2F1.ts&proxy=socks5%3A%2F%2F127.0.0.1%3A1080",
            "http://127.0.0.1:7860/segment/abc/1.ts",
            "http://127.0.0.1:7860/decrypt/segment.mp4?url=aGVsbG8",
        ] {
            assert!(route.allows_relay_target(&url(allowed)), "{allowed}");
        }

        for blocked in [
            "http://127.0.0.1:7860/api/admin/config",
            "http://127.0.0.1:7860/admin",
            "http://127.0.0.1:7860/keyring",
            "http://127.0.0.1:7860/proxy/stream?url=http%3A%2F%2F127.0.0.1%3A22%2F",
            "http://127.0.0.1:7860/proxy/stream?d=http%3A%2F%2Flocalhost%3A8080%2F",
            "http://127.0.0.1:7860/proxy/stream?url=http%3A%2F%2F%5B%3A%3A1%5D%2F",
            "http://127.0.0.1:7860/proxy/stream?url=http%3A%2F%2F169.254.169.254%2Flatest",
            "http://127.0.0.1:7860/key?key_url=http%3A%2F%2F127.0.0.1%3A22%2F",
            "http://127.0.0.1:7860/proxy/hls/manifest.m3u8?d=https%3A%2F%2Fcdn.example.com%2Fa.m3u8&orig_url=http%3A%2F%2Flocalhost%2F",
        ] {
            assert!(!route.allows_relay_target(&url(blocked)), "{blocked}");
        }
    }

    #[test]
    fn log_redaction_masks_header_params_and_passwords_in_nested_urls() {
        let routed = route("http://127.0.0.1:7860", None).route(
            "https://cdn.example.com/a.m3u8?sig=keep",
            &HashMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
        );
        let relay_url = crate::source::hls_relay::build_relay_url(
            "127.0.0.1",
            14002,
            "session-1",
            &format!("{}&api_password=hunter2", routed.url),
        );

        let redacted = redact_url_for_log(&relay_url);
        assert!(!redacted.contains("secret"), "{redacted}");
        assert!(!redacted.contains("hunter2"), "{redacted}");
        assert!(redacted.contains("sig%253Dkeep") || redacted.contains("sig%3Dkeep"));
        assert!(redacted.contains("session-1"));

        assert_eq!(
            redact_url_for_log("https://cdn.example.com/a.m3u8"),
            "https://cdn.example.com/a.m3u8"
        );
    }
}
