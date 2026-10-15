use super::*;

use std::sync::PoisonError;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(super) struct RelayControlTarget {
    pub(super) base_url: String,
    pub(super) remote_token: String,
    pub(super) selected_peer: RelayPeerMetadata,
}

/// The peer-side address of a relay-backed session.
///
/// Unlike `RemoteSessionBacking`, this carries no local heartbeat sender, so
/// holding one across an `.await` never keeps a removed session's keepalive
/// task running.
#[derive(Debug, Clone)]
pub(super) struct RemoteSessionTarget {
    pub(super) base_url: String,
    pub(super) session_id: String,
    pub(super) token: String,
}

impl From<&RemoteSessionBacking> for RemoteSessionTarget {
    fn from(remote: &RemoteSessionBacking) -> Self {
        Self {
            base_url: remote.remote_base_url.clone(),
            session_id: remote.remote_session_id.clone(),
            token: remote.remote_token.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RelayPingResponse {
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    gpu_ready: Option<bool>,
    #[serde(default)]
    gpu_vendor: Option<String>,
    #[serde(default)]
    gpu_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RemoteStatusResponse {
    #[serde(default)]
    pub(super) gpu_ready: Option<bool>,
    #[serde(default)]
    pub(super) gpu_name: Option<String>,
    #[serde(default)]
    pub(super) gpu_vendor: Option<String>,
    #[serde(default)]
    pub(super) gpu_utilization: Option<u8>,
    #[serde(default)]
    pub(super) encoder_backend: Option<String>,
    #[serde(default)]
    pub(super) selected_executor: Option<ExecutorKind>,
    #[serde(default)]
    pub(super) nvidia_ai_available: Option<bool>,
    #[serde(default)]
    pub(super) amd_ai_available: Option<bool>,
    #[serde(default)]
    pub(super) sessions: Vec<SessionInfo>,
    #[serde(default)]
    pub(super) primary_session: Option<SessionInfo>,
    #[serde(default)]
    pub(super) encoder_has_framegen: Option<bool>,
    #[serde(default)]
    pub(super) encoder_has_native_double: Option<bool>,
    #[serde(default)]
    pub(super) encoder_has_truehdr: Option<bool>,
    #[serde(default)]
    pub(super) encoder_has_hdr_tonemap: Option<bool>,
    #[serde(default)]
    pub(super) encoder_has_rife: Option<bool>,
}

pub(super) fn relay_peer_from_ping(ip: String, ping: RelayPingResponse) -> RelayPeerMetadata {
    RelayPeerMetadata {
        instance_id: normalize_optional_text(ping.instance_id),
        hostname: normalize_optional_text(ping.hostname).or_else(|| Some("unknown".to_string())),
        ip: Some(ip),
        version: normalize_optional_text(ping.version).or_else(|| Some("unknown".to_string())),
        platform: normalize_optional_text(ping.platform),
        gpu_ready: ping.gpu_ready,
        gpu_vendor: normalize_optional_text(ping.gpu_vendor),
        gpu_name: normalize_optional_text(ping.gpu_name),
    }
}

pub(super) fn saved_relay_peer_metadata(relay: &RelaySettings) -> Option<RelayPeerMetadata> {
    let mut peer = relay.last_known_peer.clone().unwrap_or_default();

    if peer.instance_id.is_none() {
        peer.instance_id = relay.linked_peer_id.clone();
    }
    if peer.hostname.is_none() {
        peer.hostname = relay.linked_peer_hostname.clone();
    }
    if peer.ip.is_none() {
        peer.ip = relay.linked_peer_ip.clone();
    }

    (peer.instance_id.is_some()
        || peer.hostname.is_some()
        || peer.ip.is_some()
        || peer.version.is_some()
        || peer.platform.is_some()
        || peer.gpu_ready.is_some()
        || peer.gpu_vendor.is_some()
        || peer.gpu_name.is_some())
    .then_some(peer)
}

pub(super) fn relay_identity_matches_link(
    saved_peer: &RelayPeerMetadata,
    live_peer: &RelayPeerMetadata,
) -> bool {
    if let Some(expected_instance_id) = saved_peer.instance_id.as_deref() {
        return live_peer.instance_id.as_deref() == Some(expected_instance_id);
    }

    saved_peer.ip.as_deref() == live_peer.ip.as_deref()
}

pub(super) async fn probe_relay_peer(ip: &str) -> Result<RelayPeerMetadata, String> {
    let url = format!("http://{}:{}/v1/ping", ip, PORT);
    let response = relay_control_http_client()?
        .get(&url)
        .timeout(Duration::from_millis(RELAY_STATUS_TIMEOUT_MS))
        .send()
        .await
        .map_err(|error| format!("Could not reach {}: {}", ip, error))?;

    if !response.status().is_success() {
        return Err(format!("{} responded with HTTP {}.", ip, response.status()));
    }

    let ping = response
        .json::<RelayPingResponse>()
        .await
        .map_err(|error| format!("{} returned an invalid status response: {}", ip, error))?;

    Ok(relay_peer_from_ping(ip.to_string(), ping))
}

pub(super) fn relay_target_display_name(peer: &RelayPeerMetadata) -> &str {
    peer.hostname
        .as_deref()
        .or(peer.ip.as_deref())
        .unwrap_or("the linked REFEREE instance")
}

pub(super) fn is_supported_local_gpu_vendor(gpu_vendor: &str) -> bool {
    matches!(
        gpu_vendor.trim().to_ascii_lowercase().as_str(),
        "nvidia" | "amd"
    )
}

pub(super) fn has_supported_local_gpu(gpu: &GpuInfo) -> bool {
    is_supported_local_gpu_vendor(&gpu.vendor) && gpu.backend.is_some()
}

pub(super) fn local_gpu_ready_for_status(gpu: &GpuInfo, setup_complete: bool) -> bool {
    has_supported_local_gpu(gpu) && setup_complete
}

// ── Linked peer reachability ─────────────────────────────────────────────────

/// The saved link a check runs against: who the peer should be, where it was
/// last seen, and the token that authorizes control calls.
#[derive(Debug, Clone)]
pub(super) struct SavedRelayLink {
    pub(super) peer: RelayPeerMetadata,
    pub(super) ip: String,
    pub(super) token: String,
}

impl SavedRelayLink {
    pub(super) fn from_settings(relay: &RelaySettings) -> Result<Self, String> {
        let peer = saved_relay_peer_metadata(relay).ok_or_else(|| {
            "REFEREE Relay is enabled, but no linked peer is configured.".to_string()
        })?;
        let ip = normalize_optional_text(peer.ip.clone()).ok_or_else(|| {
            "REFEREE Relay is enabled, but the linked peer is missing an IP address.".to_string()
        })?;
        let token = normalize_optional_text(relay.remote_token.clone()).ok_or_else(|| {
            "REFEREE Relay is enabled, but no saved relay control token was found.".to_string()
        })?;

        Ok(Self { peer, ip, token })
    }

    fn key(&self) -> RelayLinkKey {
        RelayLinkKey {
            instance_id: self.peer.instance_id.clone(),
            ip: self.ip.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelayLinkKey {
    instance_id: Option<String>,
    ip: String,
}

#[derive(Debug, Clone)]
struct RelayLinkSnapshot {
    key: RelayLinkKey,
    probed_at: Instant,
    outcome: Result<RelayPeerMetadata, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelayLinkFreshness {
    /// Status polling: answer from the last probe, and refresh it in the
    /// background once it is older than `RELAY_LINK_PROBE_TTL_MS`.
    Cached,
    /// Before the relay token is sent to the peer: probe now.
    Fresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebindScan {
    /// At most once per `RELAY_REBIND_SCAN_INTERVAL_SECS`.
    Throttled,
    /// A user is waiting on this answer (stream start), so scan regardless.
    Now,
}

/// Shared view of the linked relay peer's reachability.
///
/// Every status producer (`/v1/status`, `status-update` events, the desktop
/// status loop, `get_linked_relay_status`) reads through this cache. Many
/// pollers then share one `/v1/ping` probe, and an offline peer costs one
/// background probe per TTL instead of stalling every status call for the
/// probe timeout.
///
/// A failed probe also drives rebinding. When the peer stops answering at its
/// saved address, the local subnet is scanned for the same instance ID (a DHCP
/// move) and the new address is written back to the relay settings.
pub struct RelayLinkMonitor {
    snapshot: std::sync::Mutex<Option<RelayLinkSnapshot>>,
    probe_lock: tokio::sync::Mutex<()>,
    refreshing: AtomicBool,
    last_rebind_scan: std::sync::Mutex<Option<Instant>>,
    rebind_scans: bool,
}

impl Default for RelayLinkMonitor {
    fn default() -> Self {
        Self {
            snapshot: std::sync::Mutex::new(None),
            probe_lock: tokio::sync::Mutex::new(()),
            refreshing: AtomicBool::new(false),
            last_rebind_scan: std::sync::Mutex::new(None),
            rebind_scans: true,
        }
    }
}

/// Clears the background-refresh flag even if the refresh task unwinds.
struct ClearOnDrop<'a>(&'a AtomicBool);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl RelayLinkMonitor {
    /// A monitor that never scans the subnet, for tests that must not touch the LAN.
    #[cfg(test)]
    pub(crate) fn without_rebind_scans() -> Self {
        Self {
            rebind_scans: false,
            ..Self::default()
        }
    }

    pub(super) async fn check_link(
        self: &Arc<Self>,
        state: &ServerState,
        link: &SavedRelayLink,
        freshness: RelayLinkFreshness,
    ) -> Result<RelayPeerMetadata, String> {
        let requested_at = Instant::now();

        if freshness == RelayLinkFreshness::Cached {
            if let Some(snapshot) = self.snapshot_for(&link.key()) {
                if snapshot.probed_at.elapsed() >= Duration::from_millis(RELAY_LINK_PROBE_TTL_MS) {
                    self.spawn_refresh(state, link);
                }
                return snapshot.outcome;
            }
        }

        let rebind = match freshness {
            RelayLinkFreshness::Cached => RebindScan::Throttled,
            RelayLinkFreshness::Fresh => RebindScan::Now,
        };
        self.refresh(state, link, rebind, requested_at).await
    }

    fn snapshot_for(&self, key: &RelayLinkKey) -> Option<RelayLinkSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|snapshot| &snapshot.key == key)
            .cloned()
    }

    fn store(&self, key: RelayLinkKey, outcome: Result<RelayPeerMetadata, String>) {
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner) = Some(RelayLinkSnapshot {
            key,
            probed_at: Instant::now(),
            outcome,
        });
    }

    fn spawn_refresh(self: &Arc<Self>, state: &ServerState, link: &SavedRelayLink) {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }

        let monitor = Arc::clone(self);
        let state = state.clone();
        let link = link.clone();
        tokio::spawn(async move {
            let _clear = ClearOnDrop(&monitor.refreshing);
            let _ = monitor
                .refresh(&state, &link, RebindScan::Throttled, Instant::now())
                .await;
        });
    }

    async fn refresh(
        &self,
        state: &ServerState,
        link: &SavedRelayLink,
        rebind: RebindScan,
        accept_since: Instant,
    ) -> Result<RelayPeerMetadata, String> {
        let _probe = self.probe_lock.lock().await;

        // A probe that finished while this call waited for the lock is as good
        // as a new one, so concurrent callers share a single round trip.
        if let Some(snapshot) = self.snapshot_for(&link.key()) {
            if snapshot.probed_at >= accept_since {
                return snapshot.outcome;
            }
        }

        let outcome = match probe_relay_peer(&link.ip).await {
            Ok(live_peer) if relay_identity_matches_link(&link.peer, &live_peer) => Ok(live_peer),
            Ok(_) => Err(
                "A different REFEREE instance responded at the linked relay address.".to_string(),
            ),
            Err(error) => Err(error),
        };
        let outcome = match outcome {
            Err(error) => self.find_moved_peer(state, link, rebind).await.ok_or(error),
            ok => ok,
        };

        let key = match &outcome {
            Ok(live_peer) => RelayLinkKey {
                instance_id: link.peer.instance_id.clone(),
                ip: live_peer.ip.clone().unwrap_or_else(|| link.ip.clone()),
            },
            Err(_) => link.key(),
        };
        self.store(key, outcome.clone());
        outcome
    }

    async fn find_moved_peer(
        &self,
        state: &ServerState,
        link: &SavedRelayLink,
        rebind: RebindScan,
    ) -> Option<RelayPeerMetadata> {
        // Links saved before peers reported instance IDs have nothing stable to
        // match on, so they cannot be rebound.
        link.peer.instance_id.as_ref()?;
        if !self.rebind_scans || !self.claim_rebind_scan(rebind) {
            return None;
        }

        let scanned = scan_subnet_for_relay_peers(Some(&state.instance_id)).await;
        let moved = select_moved_relay_peer(&link.peer, scanned)?;
        adopt_relay_peer_address(state, &moved)
            .await
            .then_some(moved)
    }

    fn claim_rebind_scan(&self, rebind: RebindScan) -> bool {
        let mut last_scan = self
            .last_rebind_scan
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let throttled = rebind == RebindScan::Throttled
            && last_scan.is_some_and(|at| {
                now.duration_since(at) < Duration::from_secs(RELAY_REBIND_SCAN_INTERVAL_SECS)
            });

        if !throttled {
            *last_scan = Some(now);
        }
        !throttled
    }
}

/// Probes `GET /v1/ping` on every other host in the local IPv4 /24 and returns
/// the REFEREE instances that answered, excluding this one.
pub(crate) async fn scan_subnet_for_relay_peers(
    local_instance_id: Option<&str>,
) -> Vec<RelayPeerMetadata> {
    let std::net::IpAddr::V4(local_ip) = local_ip() else {
        return Vec::new();
    };
    if local_ip.is_loopback() {
        return Vec::new();
    }
    let Ok(client) = relay_control_http_client() else {
        return Vec::new();
    };

    let [a, b, c, local_host] = local_ip.octets();
    let tasks: Vec<_> = (1u8..=254)
        .filter(|&host| host != local_host)
        .map(|host| {
            let ip = format!("{}.{}.{}.{}", a, b, c, host);
            tokio::spawn(async move {
                let response = client
                    .get(format!("http://{}:{}/v1/ping", ip, PORT))
                    .timeout(Duration::from_millis(RELAY_DISCOVERY_TIMEOUT_MS))
                    .send()
                    .await
                    .ok()
                    .filter(|response| response.status().is_success())?;
                let ping = response.json::<RelayPingResponse>().await.ok()?;
                Some(relay_peer_from_ping(ip, ping))
            })
        })
        .collect();

    let mut peers = Vec::new();
    for task in tasks {
        if let Ok(Some(peer)) = task.await {
            if local_instance_id.is_none() || peer.instance_id.as_deref() != local_instance_id {
                peers.push(peer);
            }
        }
    }
    peers
}

/// Picks the scanned host that is the linked instance at its current address.
///
/// Instance IDs are self-reported and the chosen host receives the relay token,
/// so responders that disagree about who they are (different hostnames under
/// the same ID) are treated as "not found" rather than guessed between. A
/// single machine answering on several addresses (wired and Wi-Fi) is fine and
/// resolves to the lowest address, matching desktop discovery's de-duplication.
pub(super) fn select_moved_relay_peer(
    saved_peer: &RelayPeerMetadata,
    scanned: impl IntoIterator<Item = RelayPeerMetadata>,
) -> Option<RelayPeerMetadata> {
    let instance_id = saved_peer.instance_id.as_deref()?;
    let mut candidates: Vec<_> = scanned
        .into_iter()
        .filter(|peer| peer.instance_id.as_deref() == Some(instance_id) && peer.ip.is_some())
        .collect();
    candidates.sort_by(|a, b| a.ip.cmp(&b.ip));

    let first = candidates.first()?;
    if candidates
        .iter()
        .any(|candidate| candidate.hostname != first.hostname)
    {
        warn!(
            "[Relay]: Hosts with different names claim linked instance {}; not rebinding.",
            instance_id
        );
        return None;
    }

    candidates.into_iter().next()
}

/// Rewrites the saved address of the linked peer. Returns `false`, changing
/// nothing, when the link no longer targets `peer`'s instance.
pub(super) fn retarget_relay_settings(relay: &mut RelaySettings, peer: &RelayPeerMetadata) -> bool {
    let linked_instance_id = saved_relay_peer_metadata(relay).and_then(|saved| saved.instance_id);
    if !relay.enabled || linked_instance_id.is_none() || linked_instance_id != peer.instance_id {
        return false;
    }

    relay.linked_peer_ip = peer.ip.clone();
    if peer.hostname.is_some() {
        relay.linked_peer_hostname = peer.hostname.clone();
    }
    relay.last_known_peer = Some(peer.clone());
    true
}

/// Points the relay link at `peer`'s current address when `peer` is the linked
/// instance, persisting the change and syncing it to the desktop UI.
///
/// Returns `true` when the link targets `peer` afterward (including when it
/// already did, which writes nothing), and `false` when the link was unlinked or
/// relinked to another instance in the meantime.
pub(super) async fn adopt_relay_peer_address(
    state: &ServerState,
    peer: &RelayPeerMetadata,
) -> bool {
    let Some(ip) = peer.ip.as_deref() else {
        return false;
    };

    {
        let current = state.settings.read().await;
        let saved = saved_relay_peer_metadata(&current.relay);
        let linked = current.relay.enabled
            && saved
                .as_ref()
                .and_then(|saved| saved.instance_id.as_ref())
                .is_some_and(|instance_id| peer.instance_id.as_ref() == Some(instance_id));
        if !linked {
            return false;
        }
        if saved.and_then(|saved| saved.ip).as_deref() == Some(ip) {
            return true;
        }
    }

    info!(
        "[Relay]: Linked peer {} answered at {}; updating the saved relay address.",
        relay_target_display_name(peer),
        ip
    );

    let settings_manager = state.settings_manager.clone().or_else(|| {
        state
            .settings_path
            .as_ref()
            .map(|path| Arc::new(SettingsManager::new(path)))
    });
    let Some(settings_manager) = settings_manager else {
        return retarget_relay_settings(&mut state.settings.write().await.relay, peer);
    };

    let mut settings = settings_manager.get();
    if !retarget_relay_settings(&mut settings.relay, peer) {
        return false;
    }
    let updated = settings_manager.update(settings);
    *state.settings.write().await = ServerSettings::from(&updated);

    #[cfg(not(feature = "headless"))]
    if let Some(app_handle) = state.app_handle.as_ref() {
        let _ = app_handle.emit("settings-sync", &updated);
    }

    true
}

/// Repoints the relay link when a subnet scan (such as desktop discovery) found
/// the linked instance at a new address.
#[cfg(not(feature = "headless"))]
pub(crate) async fn adopt_moved_relay_peer(state: &ServerState, scanned: Vec<RelayPeerMetadata>) {
    let saved_peer = saved_relay_peer_metadata(&state.settings.read().await.relay);
    if let Some(moved) =
        saved_peer.and_then(|saved_peer| select_moved_relay_peer(&saved_peer, scanned))
    {
        adopt_relay_peer_address(state, &moved).await;
    }
}

/// Resolves the linked relay peer, verified against the saved instance ID (or
/// saved IP for links that predate instance IDs).
pub(crate) async fn linked_relay_peer(
    state: &ServerState,
    relay: &RelaySettings,
    freshness: RelayLinkFreshness,
) -> Result<RelayPeerMetadata, String> {
    let link = SavedRelayLink::from_settings(relay)?;
    state.relay_link.check_link(state, &link, freshness).await
}

pub(super) async fn relay_gpu_ready_for_status(
    state: &ServerState,
    relay: &RelaySettings,
) -> Option<bool> {
    if !relay.enabled {
        return None;
    }

    let peer = linked_relay_peer(state, relay, RelayLinkFreshness::Cached).await;
    Some(peer.is_ok_and(|peer| peer.gpu_ready.unwrap_or(false)))
}

pub(super) async fn gpu_ready_for_status(
    state: &ServerState,
    gpu: &GpuInfo,
    settings: &ServerSettings,
    setup_complete: bool,
) -> bool {
    if let Some(relay_ready) = relay_gpu_ready_for_status(state, &settings.relay).await {
        return relay_ready;
    }

    local_gpu_ready_for_status(gpu, setup_complete)
}

pub(super) async fn resolve_active_relay_target(
    state: &ServerState,
    relay: &RelaySettings,
) -> Result<Option<RelayControlTarget>, String> {
    if !relay.enabled {
        return Ok(None);
    }

    let link = SavedRelayLink::from_settings(relay)?;
    let live_peer = state
        .relay_link
        .check_link(state, &link, RelayLinkFreshness::Fresh)
        .await?;

    if live_peer.gpu_ready == Some(false) {
        return Err(format!(
            "{} is online, but it is not ready to process streams yet.",
            relay_target_display_name(&live_peer)
        ));
    }

    let ip = live_peer.ip.clone().unwrap_or(link.ip);
    Ok(Some(RelayControlTarget {
        base_url: format!("http://{}:{}", ip, PORT),
        remote_token: link.token,
        selected_peer: live_peer,
    }))
}

// ── Remote session control ───────────────────────────────────────────────────

pub(super) fn local_output_url(session_id: &str) -> String {
    format!("http://localhost:{}/v1/tmp/{}/index.m3u8", PORT, session_id)
}

pub(super) async fn relay_response_error_message(
    response: reqwest::Response,
    fallback: impl Into<String>,
) -> String {
    let fallback = fallback.into();
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();

    if let Ok(payload) = serde_json::from_slice::<ErrorResponse>(&body) {
        let message = payload.error.trim();
        if !message.is_empty() {
            return message.to_string();
        }
    }

    if let Ok(text) = std::str::from_utf8(&body) {
        let trimmed = text.trim();
        if !trimmed.is_empty() && trimmed.len() <= 256 {
            return trimmed.to_string();
        }
    }

    format!("{} (HTTP {}).", fallback, status)
}

pub(super) async fn forward_relay_request(
    url: String,
    remote_token: &str,
    timeout_secs: u64,
    configure: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    send_error_message: &str,
) -> Result<reqwest::Response, String> {
    let request = relay_control_http_client()?
        .post(url)
        .header("X-Referee-Token", remote_token);

    configure(request)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|error| format!("{}: {}", send_error_message, error))
}

pub(super) async fn forward_remote_stream_start(
    request: &StreamStartRequest,
    relay_target: &RelayControlTarget,
) -> Result<StreamStartResponse, String> {
    let response = forward_relay_request(
        format!("{}/v1/stream/start", relay_target.base_url),
        relay_target.remote_token.as_str(),
        RELAY_STREAM_START_TIMEOUT_SECS,
        |builder| builder.json(request),
        "Failed to start the stream on the linked REFEREE instance",
    )
    .await?;

    if response.status().is_success() {
        return response
            .json::<StreamStartResponse>()
            .await
            .map_err(|error| {
                format!(
                    "Linked REFEREE instance returned an invalid start response: {}",
                    error
                )
            });
    }

    Err(relay_response_error_message(
        response,
        "Linked REFEREE instance rejected the stream start request",
    )
    .await)
}

/// Reads the just-started remote session from the peer's status.
///
/// The remote stream is already running (possibly after minutes of startup), so
/// a transient miss is retried before the caller gives up and rolls it back.
pub(super) async fn fetch_remote_session_info(
    relay_target: &RelayControlTarget,
    remote_session_id: &str,
) -> Result<SessionInfo, String> {
    let mut attempt = 1;
    loop {
        let result = fetch_remote_status(
            &relay_target.base_url,
            Duration::from_secs(RELAY_SESSION_STATUS_TIMEOUT_SECS),
        )
        .await
        .and_then(|mut status| {
            take_remote_session_info(&mut status, remote_session_id).ok_or_else(|| {
                format!(
                    "Linked REFEREE instance started session {}, but it was not present in relay status.",
                    remote_session_id
                )
            })
        });

        match result {
            Err(error) if attempt < RELAY_STATUS_SYNC_ATTEMPTS => {
                debug!(
                    "[Relay]: Status sync for remote session {} failed (attempt {}/{}): {}",
                    remote_session_id, attempt, RELAY_STATUS_SYNC_ATTEMPTS, error
                );
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(RELAY_STATUS_SYNC_RETRY_DELAY_MS)).await;
            }
            result => return result,
        }
    }
}

pub(super) async fn fetch_remote_status(
    remote_base_url: &str,
    timeout: Duration,
) -> Result<RemoteStatusResponse, String> {
    let response = relay_control_http_client()?
        .get(format!("{}/v1/status", remote_base_url))
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("Failed to query relay session status: {}", error))?;

    if !response.status().is_success() {
        return Err(relay_response_error_message(
            response,
            "Linked REFEREE instance rejected the session status request",
        )
        .await);
    }

    response
        .json::<RemoteStatusResponse>()
        .await
        .map_err(|error| {
            format!(
                "Linked REFEREE instance returned an invalid status payload: {}",
                error
            )
        })
}

/// Takes the peer's record for `remote_session_id` out of its status payload.
///
/// Matches by ID only. The peer may be running some other client's session, and
/// presenting that as this one would show the wrong stream in local status.
pub(super) fn take_remote_session_info(
    status: &mut RemoteStatusResponse,
    remote_session_id: &str,
) -> Option<SessionInfo> {
    if let Some(index) = status
        .sessions
        .iter()
        .position(|session| session.id == remote_session_id)
    {
        return Some(status.sessions.remove(index));
    }

    status
        .primary_session
        .take()
        .filter(|session| session.id == remote_session_id)
}

pub(super) async fn forward_remote_heartbeat_request(
    remote: &RemoteSessionTarget,
) -> Result<bool, String> {
    let response = forward_relay_request(
        format!(
            "{}/v1/stream/heartbeat/{}",
            remote.base_url, remote.session_id
        ),
        remote.token.as_str(),
        RELAY_SESSION_CONTROL_TIMEOUT_SECS,
        |builder| builder,
        "Failed to heartbeat the linked REFEREE instance",
    )
    .await?;

    if response.status().is_success() {
        return Ok(true);
    }

    if response.status() == StatusCode::NOT_FOUND {
        return Ok(false);
    }

    Err(relay_response_error_message(
        response,
        "Linked REFEREE instance rejected the heartbeat request",
    )
    .await)
}

pub(super) async fn forward_remote_stop_request(
    remote: &RemoteSessionTarget,
) -> Result<bool, String> {
    let body = StopRequest {
        session_id: Some(remote.session_id.clone()),
        stop_all: false,
    };
    let response = forward_relay_request(
        format!("{}/v1/stream/stop", remote.base_url),
        remote.token.as_str(),
        RELAY_SESSION_CONTROL_TIMEOUT_SECS,
        |builder| builder.json(&body),
        "Failed to stop the linked REFEREE session",
    )
    .await?;

    if response.status().is_success() {
        return Ok(true);
    }

    if response.status() == StatusCode::NOT_FOUND {
        return Ok(false);
    }

    Err(relay_response_error_message(
        response,
        "Linked REFEREE instance rejected the stop request",
    )
    .await)
}

/// Tells the peer to stop a remote session without letting a failure block the
/// caller. Callers drop the local record first, which ends its keepalive, so an
/// unreachable peer still reaps the session once its heartbeat timeout passes.
pub(super) async fn stop_remote_session(remote: &RemoteSessionTarget) {
    if let Err(error) = forward_remote_stop_request(remote).await {
        warn!(
            "[Relay]: Could not stop remote session {} on the linked peer; it will expire there after missing heartbeats: {}",
            remote.session_id, error
        );
    }
}

pub(super) async fn rollback_remote_session_start(
    relay_target: &RelayControlTarget,
    remote_session_id: &str,
) {
    stop_remote_session(&RemoteSessionTarget {
        base_url: relay_target.base_url.clone(),
        session_id: remote_session_id.to_string(),
        token: relay_target.remote_token.clone(),
    })
    .await;
}

/// Stops a tracked session and returns its app name.
///
/// Relay-backed sessions are removed locally first, and then the peer is told
/// to stop. That call is best-effort: once the local record is gone its
/// keepalive stops heartbeating the peer, so the peer reaps the remote session
/// within its heartbeat timeout even when it is unreachable now. An offline
/// peer therefore can never wedge a stop, or the eviction a new start does.
pub async fn stop_tracked_session(session_id: &str, sessions: &SessionMap) -> Option<String> {
    let (app_name, remote) = {
        let session = sessions.get(session_id)?;
        (
            session.info.app_name.clone(),
            session.remote_backing().map(RemoteSessionTarget::from),
        )
    };

    cleanup_session(session_id, sessions).await;
    if let Some(remote) = remote {
        stop_remote_session(&remote).await;
    }
    app_name
}

/// Stops all sessions currently tracked in the server state and returns their IDs.
pub(super) async fn stop_existing_sessions(state: &ServerState) -> Vec<String> {
    let existing_ids: Vec<String> = state.sessions.iter().map(|e| e.key().clone()).collect();
    for existing_id in &existing_ids {
        info!(
            "[Pipeline]: Stopping existing session {} before starting new stream",
            existing_id
        );
        stop_tracked_session(existing_id, &state.sessions).await;
    }
    existing_ids
}

// ── Relay session keepalive ──────────────────────────────────────────────────

/// Keeps a relay-backed session's remote half alive for as long as its local
/// client is.
///
/// Client heartbeats only reach this task through the session's channel; it
/// owns the peer round trip. That decouples the client from LAN latency, and
/// gives the peer a keepalive every `RELAY_KEEPALIVE_INTERVAL_MS`, which leaves
/// several attempts inside the peer's 15 s timeout, where a forwarded 10 s
/// client heartbeat left none to spare. The task also applies the local
/// pipeline's heartbeat contract on this side, and it ends the local record
/// when the remote session is gone or the peer stays unreachable.
pub(super) fn spawn_relay_session_keepalive(
    state: ServerState,
    local_session_id: String,
    remote: RemoteSessionTarget,
    heartbeat_rx: mpsc::Receiver<()>,
) {
    tokio::spawn(run_relay_session_keepalive(
        state,
        local_session_id,
        remote,
        heartbeat_rx,
    ));
}

async fn run_relay_session_keepalive(
    state: ServerState,
    local_session_id: String,
    remote: RemoteSessionTarget,
    mut heartbeat_rx: mpsc::Receiver<()>,
) {
    let started_at = Instant::now();
    let mut last_client_heartbeat: Option<Instant> = None;
    let mut last_peer_ack = Instant::now();
    let mut ticker = tokio::time::interval(Duration::from_millis(RELAY_KEEPALIVE_INTERVAL_MS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            message = heartbeat_rx.recv() => match message {
                Some(()) => {
                    last_client_heartbeat = Some(Instant::now());
                    continue;
                }
                // The local record was removed. Whoever removed it owns the remote stop.
                None => return,
            },
            _ = ticker.tick() => {}
        }

        if let Some(reason) = relay_client_expiry(started_at, last_client_heartbeat, Instant::now())
        {
            info!(
                "[Relay]: Session {} {}; stopping remote session {}.",
                local_session_id, reason, remote.session_id
            );
            end_relay_session(&state, &local_session_id, Some(&remote)).await;
            return;
        }

        match forward_remote_heartbeat_request(&remote).await {
            Ok(true) => last_peer_ack = Instant::now(),
            Ok(false) => {
                warn!(
                    "[Relay]: Remote session {} for session {} is no longer active on the linked peer.",
                    remote.session_id, local_session_id
                );
                end_relay_session(&state, &local_session_id, None).await;
                return;
            }
            Err(error)
                if last_peer_ack.elapsed() >= Duration::from_millis(RELAY_PEER_LOSS_TIMEOUT_MS) =>
            {
                warn!(
                    "[Relay]: Lost the linked peer for session {} after {} ms without a keepalive acknowledgement: {}",
                    local_session_id, RELAY_PEER_LOSS_TIMEOUT_MS, error
                );
                end_relay_session(&state, &local_session_id, Some(&remote)).await;
                return;
            }
            Err(error) => {
                debug!(
                    "[Relay]: Keepalive for session {} failed; retrying: {}",
                    local_session_id, error
                );
            }
        }
    }
}

/// Applies the local pipeline's heartbeat contract to a relay-backed session:
/// the first client heartbeat within `ORPHAN_SESSION_TIMEOUT_MS` of start, and
/// each later one within `HEARTBEAT_TIMEOUT_MS`.
pub(super) fn relay_client_expiry(
    started_at: Instant,
    last_client_heartbeat: Option<Instant>,
    now: Instant,
) -> Option<&'static str> {
    match last_client_heartbeat {
        Some(at) if now.duration_since(at) >= Duration::from_millis(HEARTBEAT_TIMEOUT_MS) => {
            Some("timed out due to client inactivity")
        }
        None if now.duration_since(started_at)
            >= Duration::from_millis(ORPHAN_SESSION_TIMEOUT_MS) =>
        {
            Some("timed out waiting for the first client heartbeat")
        }
        _ => None,
    }
}

async fn end_relay_session(
    state: &ServerState,
    local_session_id: &str,
    stop_remote: Option<&RemoteSessionTarget>,
) {
    cleanup_session(local_session_id, &state.sessions).await;
    emit_status_update(state).await;
    if let Some(remote) = stop_remote {
        stop_remote_session(remote).await;
    }
}

pub(super) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}
