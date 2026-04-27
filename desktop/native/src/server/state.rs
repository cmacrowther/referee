use super::*;

// ── Remote-address extractor ─────────────────────────────────────────────────

/// Extracts the remote socket address from `ConnectInfo` if available, falling
/// back to `"unknown"` in contexts where connect info is not configured (tests,
/// non-TCP transports).  Using a custom extractor avoids the `Option<ConnectInfo>`
/// pattern which has Handler trait bound issues in Axum 0.8 without connect info.
pub(super) struct RemoteIp(pub(super) String);

impl<S> axum::extract::FromRequestParts<S> for RemoteIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Ok(RemoteIp(ip))
    }
}

// ── Per-IP rate limiter ──────────────────────────────────────────────────────

/// Simple fixed-window rate limiter keyed by an arbitrary string (typically `"ip:path"`).
///
/// Each window is independent per key. Returns `false` (i.e. the call should be
/// rejected) when the caller has exceeded `max_per_window` calls in the current
/// `window_secs`-second window.
#[derive(Debug)]
pub(super) struct RateLimitBucket {
    count: u32,
    window_start: std::time::Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct RateLimiter {
    buckets: Arc<DashMap<String, RateLimitBucket>>,
    max_per_window: u32,
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_per_window: u32, window_secs: u64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            max_per_window,
            window_secs,
        }
    }

    /// Returns `true` if the request is within the rate limit (allowed).
    /// Returns `false` if the limit is exceeded.
    pub fn check(&self, key: &str) -> bool {
        let now = std::time::Instant::now();
        let window = std::time::Duration::from_secs(self.window_secs);
        let mut entry = self
            .buckets
            .entry(key.to_string())
            .or_insert(RateLimitBucket {
                count: 0,
                window_start: now,
            });
        if now.duration_since(entry.window_start) >= window {
            entry.count = 1;
            entry.window_start = now;
            true
        } else if entry.count < self.max_per_window {
            entry.count += 1;
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub struct ConsentDecision {
    pub approved: bool,
    pub always_allow: bool,
}

#[derive(Clone)]
pub struct ServerState {
    pub sessions: SessionMap,
    pub instance_id: String,
    pub gpu_info: Arc<RwLock<GpuInfo>>,
    pub settings: Arc<RwLock<ServerSettings>>,
    pub encoder_capabilities: Arc<RwLock<Option<EncoderCapabilities>>>,
    pub tmp_dir: PathBuf,
    pub app_handle: Option<DesktopAppHandle>,
    pub api_token: Arc<std::sync::RwLock<String>>,
    pub pending_consents: Arc<DashMap<String, oneshot::Sender<ConsentDecision>>>,
    pub approved_origins: Arc<DashMap<String, ApprovedOriginMeta>>,
    pub approved_origins_path: Option<PathBuf>,
    pub settings_path: Option<PathBuf>,
    pub settings_manager: Option<Arc<SettingsManager>>,
    /// Rate limiter for `POST /v1/auth/request` — caps consent-spam from LAN neighbors.
    pub rate_limit_auth: Arc<RateLimiter>,
    /// Rate limiter for `POST /v1/stream/start` — prevents GPU DoS.
    pub rate_limit_stream: Arc<RateLimiter>,
    /// The payload of the consent request currently awaiting a user decision,
    /// if any. Stored so the desktop UI can retrieve it even if it missed the
    /// initial Tauri event (e.g. the window was just created).
    pub pending_consent_ui: Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    /// Set to `true` once the first-launch setup process has completed (or was
    /// determined to be unnecessary). Local GPU readiness in status responses
    /// reflects this flag so external callers cannot mistake "GPU detected but
    /// still downloading binaries" for "ready to process streams".
    pub setup_complete: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct ServerSettings {
    pub resolution: String,
    pub quality: u8,
    pub framegen: bool,
    pub hdr: bool,
    pub executor_preference: ExecutorPreference,
    pub show_on_proxy_start: bool,
    pub notifications: bool,
    pub encoding_profiles: std::collections::HashMap<String, EncodingProfile>,
    pub relay: RelaySettings,
}

impl From<&Settings> for ServerSettings {
    fn from(s: &Settings) -> Self {
        Self {
            resolution: s.resolution.clone(),
            quality: s.quality,
            framegen: s.framegen,
            hdr: s.hdr,
            executor_preference: s.executor_preference,
            show_on_proxy_start: s.show_on_proxy_start,
            notifications: s.notifications,
            encoding_profiles: s.encoding_profiles.clone(),
            relay: s.relay.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(super) struct StreamUiSettings {
    pub(super) show_on_proxy_start: bool,
    pub(super) notifications: bool,
}

impl From<&ServerSettings> for StreamUiSettings {
    fn from(settings: &ServerSettings) -> Self {
        Self {
            show_on_proxy_start: settings.show_on_proxy_start,
            notifications: settings.notifications,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct LocalStreamSettings {
    pub(super) resolution: String,
    pub(super) quality: u8,
    pub(super) framegen: bool,
    pub(super) hdr: bool,
    pub(super) executor_preference: ExecutorPreference,
    pub(super) encoding_profiles: HashMap<String, EncodingProfile>,
    pub(super) ui: StreamUiSettings,
}

impl From<&ServerSettings> for LocalStreamSettings {
    fn from(settings: &ServerSettings) -> Self {
        Self {
            resolution: settings.resolution.clone(),
            quality: settings.quality,
            framegen: settings.framegen,
            hdr: settings.hdr,
            executor_preference: settings.executor_preference,
            encoding_profiles: settings.encoding_profiles.clone(),
            ui: StreamUiSettings::from(settings),
        }
    }
}
