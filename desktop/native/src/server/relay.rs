use super::*;

#[derive(Debug, Clone)]
pub(super) struct RelayControlTarget {
    pub(super) base_url: String,
    pub(super) remote_token: String,
    pub(super) selected_peer: RelayPeerMetadata,
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
    pub(super) encoder_has_truehdr: Option<bool>,
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
        .timeout(std::time::Duration::from_millis(RELAY_STATUS_TIMEOUT_MS))
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

pub(super) async fn relay_gpu_ready_for_status(relay: &RelaySettings) -> Option<bool> {
    if !relay.enabled {
        return None;
    }

    if relay
        .remote_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .is_none()
    {
        return Some(false);
    }

    let Some(peer) = saved_relay_peer_metadata(relay) else {
        return Some(false);
    };

    let Some(ip) = peer
        .ip
        .as_deref()
        .map(str::trim)
        .filter(|ip| !ip.is_empty())
    else {
        return Some(false);
    };

    match probe_relay_peer(ip).await {
        Ok(live_peer) if relay_identity_matches_link(&peer, &live_peer) => {
            Some(live_peer.gpu_ready.unwrap_or(false))
        }
        Ok(_) | Err(_) => Some(false),
    }
}

pub(super) async fn gpu_ready_for_status(
    gpu: &GpuInfo,
    settings: &ServerSettings,
    setup_complete: bool,
) -> bool {
    if let Some(relay_ready) = relay_gpu_ready_for_status(&settings.relay).await {
        return relay_ready;
    }

    local_gpu_ready_for_status(gpu, setup_complete)
}

pub async fn apply_status_readiness(
    mut status: StatusResponse,
    gpu: &GpuInfo,
    settings: &ServerSettings,
    setup_complete: bool,
) -> StatusResponse {
    status.gpu_ready = Some(gpu_ready_for_status(gpu, settings, setup_complete).await);
    status
}

pub(super) async fn resolve_active_relay_target(
    relay: &RelaySettings,
) -> Result<Option<RelayControlTarget>, String> {
    if !relay.enabled {
        return Ok(None);
    }

    let saved_peer = saved_relay_peer_metadata(relay)
        .ok_or_else(|| "REFEREE Relay is enabled, but no linked peer is configured.".to_string())?;
    let ip = saved_peer.ip.clone().ok_or_else(|| {
        "REFEREE Relay is enabled, but the linked peer is missing an IP address.".to_string()
    })?;
    let remote_token = relay.remote_token.clone().ok_or_else(|| {
        "REFEREE Relay is enabled, but no saved relay control token was found.".to_string()
    })?;
    let live_peer = probe_relay_peer(&ip).await?;

    if !relay_identity_matches_link(&saved_peer, &live_peer) {
        return Err(
            "A different REFEREE instance responded at the linked relay address.".to_string(),
        );
    }

    if live_peer.gpu_ready == Some(false) {
        return Err(format!(
            "{} is online, but it is not ready to process streams yet.",
            relay_target_display_name(&live_peer)
        ));
    }

    Ok(Some(RelayControlTarget {
        base_url: format!("http://{}:{}", ip, PORT),
        remote_token,
        selected_peer: live_peer,
    }))
}

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
        .timeout(std::time::Duration::from_secs(timeout_secs))
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

pub(super) async fn fetch_remote_session_info(
    relay_target: &RelayControlTarget,
    remote_session_id: &str,
) -> Result<SessionInfo, String> {
    let mut status = fetch_remote_status(&relay_target.base_url).await?;
    take_remote_session_info(&mut status, remote_session_id).ok_or_else(|| {
        format!(
            "Linked REFEREE instance started session {}, but it was not present in relay status.",
            remote_session_id
        )
    })
}

pub(super) async fn fetch_remote_status(
    remote_base_url: &str,
) -> Result<RemoteStatusResponse, String> {
    let response = relay_control_http_client()?
        .get(format!("{}/v1/status", remote_base_url))
        .timeout(std::time::Duration::from_secs(
            RELAY_SESSION_STATUS_TIMEOUT_SECS,
        ))
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

    if let Some(primary_session) = status.primary_session.take() {
        if primary_session.id == remote_session_id {
            return Some(primary_session);
        }
    }

    if status.sessions.len() == 1 {
        return Some(status.sessions.remove(0));
    }

    None
}

pub(super) async fn forward_remote_heartbeat_request(
    remote: &RemoteSessionBacking,
) -> Result<bool, String> {
    let response = forward_relay_request(
        format!(
            "{}/v1/stream/heartbeat/{}",
            remote.remote_base_url, remote.remote_session_id
        ),
        remote.remote_token.as_str(),
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
    remote_base_url: &str,
    remote_token: &str,
    remote_session_id: &str,
) -> Result<bool, String> {
    let body = StopRequest {
        session_id: Some(remote_session_id.to_string()),
        stop_all: false,
    };
    let response = forward_relay_request(
        format!("{}/v1/stream/stop", remote_base_url),
        remote_token,
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

pub(super) async fn rollback_remote_session_start(
    relay_target: &RelayControlTarget,
    remote_session_id: &str,
) {
    if let Err(error) = forward_remote_stop_request(
        &relay_target.base_url,
        &relay_target.remote_token,
        remote_session_id,
    )
    .await
    {
        warn!(
            "[Relay]: Failed to roll back remote session {} after local setup error: {}",
            remote_session_id, error
        );
    }
}

pub async fn stop_tracked_session(
    session_id: &str,
    sessions: &SessionMap,
) -> Result<Option<String>, String> {
    let Some(session) = sessions.get(session_id) else {
        return Ok(None);
    };

    let app_name = session.info.app_name.clone();
    let remote = session.remote_backing().cloned();
    drop(session);

    if let Some(remote) = remote {
        forward_remote_stop_request(
            &remote.remote_base_url,
            &remote.remote_token,
            &remote.remote_session_id,
        )
        .await?;
    }

    cleanup_session(session_id, sessions).await;
    Ok(app_name)
}

/// Stops all sessions currently tracked in the server state.
///
/// This asynchronously invokes the session cleanup routine for every session known
/// to `state`, logging each stop request.
///
/// # Examples
///
pub(super) async fn stop_existing_sessions(state: &ServerState) -> Result<Vec<String>, String> {
    let existing_ids: Vec<String> = state.sessions.iter().map(|e| e.key().clone()).collect();
    for existing_id in &existing_ids {
        info!(
            "[Pipeline]: Stopping existing session {} before starting new stream",
            existing_id
        );
        stop_tracked_session(existing_id, &state.sessions).await?;
    }
    Ok(existing_ids)
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
