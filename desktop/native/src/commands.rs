use crate::deps;
use crate::server::{self, ServerState};
use crate::settings::{PlayerSettings, RelayPeerMetadata, Settings, SettingsManager};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State, Window};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::Notify;
use tracing::info;

pub struct AppState {
    pub settings_manager: Arc<SettingsManager>,
    pub server_state: ServerState,
    pub lib_dir: PathBuf,
    pub setup_done: Arc<Notify>,
    pub setup_state: Arc<Mutex<SetupLifecycle>>,
    pub http_client: reqwest::Client,
}

#[derive(Debug, Clone, Default)]
pub struct SetupLifecycle {
    pub has_started: bool,
    pub in_progress: bool,
    pub complete: bool,
    pub progress: Option<deps::SetupProgress>,
    pub error: Option<String>,
}

const RELAY_DISCOVERY_TIMEOUT_MS: u64 = 1_000;
const RELAY_STATUS_TIMEOUT_MS: u64 = 1_500;
const RELAY_LINK_REQUEST_TIMEOUT_GRACE_SECS: u64 = 15;
const RELAY_SETTINGS_TIMEOUT_SECS: u64 = 5;

// --- Request/Response commands ---

#[tauri::command]
pub async fn get_initial_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings_manager.get())
}

#[tauri::command]
pub async fn get_boot_setting(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_app_version(app: AppHandle) -> Result<String, String> {
    Ok(app
        .config()
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string()))
}

#[tauri::command]
pub async fn get_api_token(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.server_state.api_token.read().unwrap().clone())
}

/// Returns any consent request that is currently awaiting user approval, so the
/// desktop UI can display the dialog even if it missed the initial Tauri event
/// (e.g. the window was just created when the event was emitted).
#[tauri::command]
pub async fn get_pending_consent(
    state: State<'_, AppState>,
) -> Result<Option<serde_json::Value>, String> {
    Ok(state
        .server_state
        .pending_consent_ui
        .lock()
        .unwrap()
        .clone())
}

/// Called by the desktop renderer when the user approves or denies a consent request
/// that was initiated by `POST /v1/auth/request`. The nonce ties the response back to
/// the waiting HTTP handler via a oneshot channel.
#[tauri::command]
pub async fn respond_to_consent(
    nonce: String,
    approved: bool,
    always_allow: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some((_, tx)) = state.server_state.pending_consents.remove(&nonce) {
        let _ = tx.send(server::ConsentDecision {
            approved,
            always_allow,
        });
    }
    Ok(())
}

#[derive(Serialize)]
pub struct ApprovedOriginEntry {
    pub origin: String,
    #[serde(rename = "appName")]
    pub app_name: Option<String>,
    #[serde(rename = "approvedAt")]
    pub approved_at: String,
}

/// Returns all origins that have been persistently approved ("Always Allow").
#[tauri::command]
pub async fn get_approved_origins(
    state: State<'_, AppState>,
) -> Result<Vec<ApprovedOriginEntry>, String> {
    let mut entries: Vec<ApprovedOriginEntry> = state
        .server_state
        .approved_origins
        .iter()
        .map(|e| ApprovedOriginEntry {
            origin: e.key().clone(),
            app_name: e.value().app_name.clone(),
            approved_at: e.value().approved_at.clone(),
        })
        .collect();
    // Stable order for the UI.
    entries.sort_by(|a, b| a.origin.cmp(&b.origin));
    Ok(entries)
}

/// Removes a single origin from the persistent approved-origins list and persists the change.
#[tauri::command]
pub async fn revoke_approved_origin(
    origin: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.server_state.approved_origins.remove(&origin);
    if let Some(ref path) = state.server_state.approved_origins_path {
        server::persist_approved_origins(&state.server_state.approved_origins, path);
    }
    Ok(())
}

#[derive(Serialize)]
pub struct UpdateInfo {
    #[serde(rename = "currentVersion")]
    current_version: String,
    #[serde(rename = "latestVersion")]
    latest_version: Option<String>,
    #[serde(rename = "hasUpdate")]
    has_update: bool,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateInfo, String> {
    let current_version = app
        .config()
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string());

    let client = &state.http_client;

    let response: serde_json::Value = client
        .get("https://api.github.com/repos/cmacrowther/referee/releases/latest")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let latest_version = response["tag_name"]
        .as_str()
        .map(|v| v.trim_start_matches('v').to_string());

    let has_update = latest_version
        .as_ref()
        .map(|v| !v.is_empty() && semver_gt(v, &current_version))
        .unwrap_or(false);

    let download_url = if cfg!(target_os = "windows") {
        response["assets"].as_array().and_then(|assets| {
            assets
                .iter()
                .find(|a| {
                    a["name"]
                        .as_str()
                        .map(|n| n.ends_with(".exe"))
                        .unwrap_or(false)
                })
                .and_then(|a| a["browser_download_url"].as_str().map(String::from))
        })
    } else {
        response["assets"].as_array().and_then(|assets| {
            assets
                .iter()
                .find(|a| {
                    a["name"]
                        .as_str()
                        .map(|n| n.ends_with(".AppImage"))
                        .unwrap_or(false)
                })
                .and_then(|a| a["browser_download_url"].as_str().map(String::from))
        })
    };

    Ok(UpdateInfo {
        current_version,
        latest_version,
        has_update,
        download_url,
    })
}

#[tauri::command]
pub async fn download_and_install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    download_url: String,
) -> Result<(), String> {
    let client = &state.http_client;

    let temp_dir = std::env::temp_dir();
    let temp_path = if cfg!(target_os = "windows") {
        temp_dir.join("REFEREE-Update.exe")
    } else {
        temp_dir.join("REFEREE-Update.AppImage")
    };

    let mut response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let _total = response.content_length().unwrap_or(0);

    // Stream directly to disk rather than buffering the entire installer binary in RAM.
    // NOTE: signature/hash verification is not yet implemented here; the download relies
    // on HTTPS transport integrity. A detached signature check should be added once
    // a public key is published alongside releases.
    {
        use tokio::io::AsyncWriteExt as _;
        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|e| e.to_string())?;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        file.flush().await.map_err(|e| e.to_string())?;
    }

    let _ = app.emit("update-progress", 100);

    // Launch installer and quit
    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new(&temp_path);
        crate::process::hide_std_command_window(&mut command);
        command.spawn().map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_path, perms).map_err(|e| e.to_string())?;
        std::process::Command::new(&temp_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    app.exit(0);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPeer {
    #[serde(default)]
    pub instance_id: Option<String>,
    pub ip: String,
    pub hostname: String,
    pub version: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub gpu_ready: Option<bool>,
    #[serde(default)]
    pub gpu_vendor: Option<String>,
    #[serde(default)]
    pub gpu_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedRelayStatus {
    pub linked: bool,
    pub available: Option<bool>,
    pub peer: Option<RelayPeerMetadata>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayPingResponse {
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

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn relay_peer_from_ping(ip: String, ping: RelayPingResponse) -> RelayPeer {
    RelayPeer {
        instance_id: normalize_optional_text(ping.instance_id),
        ip,
        hostname: normalize_optional_text(ping.hostname).unwrap_or_else(|| "unknown".to_string()),
        version: normalize_optional_text(ping.version).unwrap_or_else(|| "unknown".to_string()),
        platform: normalize_optional_text(ping.platform),
        gpu_ready: ping.gpu_ready,
        gpu_vendor: normalize_optional_text(ping.gpu_vendor),
        gpu_name: normalize_optional_text(ping.gpu_name),
    }
}

fn dedupe_and_filter_relay_peers(
    mut peers: Vec<RelayPeer>,
    local_instance_id: Option<&str>,
) -> Vec<RelayPeer> {
    peers.sort_by(|a, b| a.ip.cmp(&b.ip));

    let mut seen = HashSet::new();
    let mut filtered = Vec::new();

    for peer in peers {
        if local_instance_id.is_some() && peer.instance_id.as_deref() == local_instance_id {
            continue;
        }

        let dedupe_key = peer.instance_id.clone().unwrap_or_else(|| peer.ip.clone());
        if seen.insert(dedupe_key) {
            filtered.push(peer);
        }
    }

    filtered
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayAuthTokenResponse {
    token: String,
    persistent: bool,
}

#[derive(Debug, Deserialize)]
struct RelayApiErrorResponse {
    code: Option<String>,
    error: Option<String>,
}

fn relay_origin_for_instance_id(instance_id: &str) -> String {
    let sanitized_id = instance_id
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let suffix = if sanitized_id.is_empty() {
        "unknown".to_string()
    } else {
        sanitized_id
    };

    format!("https://peer-{}.referee.invalid", suffix)
}

fn relay_link_app_name() -> String {
    match sysinfo::System::host_name()
        .map(|hostname| hostname.trim().to_string())
        .filter(|hostname| !hostname.is_empty())
    {
        Some(hostname) => format!("REFEREE Relay ({})", hostname),
        None => "REFEREE Relay".to_string(),
    }
}

fn relay_peer_metadata(peer: &RelayPeer) -> crate::settings::RelayPeerMetadata {
    crate::settings::RelayPeerMetadata {
        instance_id: peer.instance_id.clone(),
        hostname: Some(peer.hostname.clone()),
        ip: Some(peer.ip.clone()),
        version: Some(peer.version.clone()),
        platform: peer.platform.clone(),
        gpu_ready: peer.gpu_ready,
        gpu_vendor: peer.gpu_vendor.clone(),
        gpu_name: peer.gpu_name.clone(),
    }
}

fn relay_peer_metadata_is_empty(peer: &RelayPeerMetadata) -> bool {
    peer.instance_id.is_none()
        && peer.hostname.is_none()
        && peer.ip.is_none()
        && peer.version.is_none()
        && peer.platform.is_none()
        && peer.gpu_ready.is_none()
        && peer.gpu_vendor.is_none()
        && peer.gpu_name.is_none()
}

fn saved_relay_peer_metadata(settings: &Settings) -> Option<RelayPeerMetadata> {
    let mut peer = settings.relay.last_known_peer.clone().unwrap_or_default();

    if peer.instance_id.is_none() {
        peer.instance_id = settings.relay.linked_peer_id.clone();
    }
    if peer.hostname.is_none() {
        peer.hostname = settings.relay.linked_peer_hostname.clone();
    }
    if peer.ip.is_none() {
        peer.ip = settings.relay.linked_peer_ip.clone();
    }

    (!relay_peer_metadata_is_empty(&peer)).then_some(peer)
}

fn relay_link_is_configured(settings: &Settings) -> bool {
    settings.relay.enabled
        && (settings.relay.linked_peer_id.is_some()
            || settings.relay.linked_peer_ip.is_some()
            || settings.relay.last_known_peer.is_some())
}

fn is_supported_setup_vendor(gpu_vendor: &str) -> bool {
    matches!(
        gpu_vendor.trim().to_ascii_lowercase().as_str(),
        "nvidia" | "amd"
    )
}

fn relay_identity_matches_link(saved_peer: &RelayPeerMetadata, live_peer: &RelayPeer) -> bool {
    if let Some(expected_instance_id) = saved_peer.instance_id.as_deref() {
        return live_peer.instance_id.as_deref() == Some(expected_instance_id);
    }

    saved_peer.ip.as_deref() == Some(live_peer.ip.as_str())
}

async fn probe_relay_peer(
    client: &reqwest::Client,
    ip: &str,
    timeout: std::time::Duration,
) -> Result<RelayPeer, String> {
    let url = format!("http://{}:14002/v1/ping", ip);
    probe_relay_peer_at_url(client, &url, ip, timeout).await
}

async fn probe_relay_peer_at_url(
    client: &reqwest::Client,
    url: &str,
    ip: &str,
    timeout: std::time::Duration,
) -> Result<RelayPeer, String> {
    let response = client
        .get(url)
        .timeout(timeout)
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

async fn resolve_linked_relay_status(
    client: &reqwest::Client,
    settings: &Settings,
) -> LinkedRelayStatus {
    let linked = relay_link_is_configured(settings);
    let Some(saved_peer) = saved_relay_peer_metadata(settings) else {
        return LinkedRelayStatus {
            linked: false,
            available: None,
            peer: None,
            reason: None,
        };
    };

    if !linked {
        return LinkedRelayStatus {
            linked: false,
            available: None,
            peer: Some(saved_peer),
            reason: None,
        };
    }

    let Some(ip) = saved_peer.ip.clone() else {
        return LinkedRelayStatus {
            linked: true,
            available: Some(false),
            peer: Some(saved_peer),
            reason: Some("Linked relay peer is missing an IP address.".to_string()),
        };
    };

    match probe_relay_peer(
        client,
        &ip,
        std::time::Duration::from_millis(RELAY_STATUS_TIMEOUT_MS),
    )
    .await
    {
        Ok(live_peer) => {
            let live_metadata = relay_peer_metadata(&live_peer);

            if !relay_identity_matches_link(&saved_peer, &live_peer) {
                return LinkedRelayStatus {
                    linked: true,
                    available: Some(false),
                    peer: Some(live_metadata),
                    reason: Some(
                        "A different REFEREE instance responded at the linked address.".to_string(),
                    ),
                };
            }

            LinkedRelayStatus {
                linked: true,
                available: Some(true),
                peer: Some(live_metadata),
                reason: None,
            }
        }
        Err(error) => LinkedRelayStatus {
            linked: true,
            available: Some(false),
            peer: Some(saved_peer),
            reason: Some(error),
        },
    }
}

fn build_linked_relay_settings(
    mut settings: Settings,
    peer: &RelayPeer,
    remote_token: String,
) -> Result<Settings, String> {
    let peer_id = peer.instance_id.clone().ok_or_else(|| {
        "Selected relay peer did not report an instance ID. Update that REFEREE installation before linking."
            .to_string()
    })?;

    settings.relay.enabled = true;
    settings.relay.linked_peer_id = Some(peer_id);
    settings.relay.linked_peer_hostname = Some(peer.hostname.clone());
    settings.relay.linked_peer_ip = Some(peer.ip.clone());
    settings.relay.remote_token = Some(remote_token);
    settings.relay.last_known_peer = Some(relay_peer_metadata(peer));
    Ok(settings)
}

fn clear_linked_relay_settings(mut settings: Settings) -> Settings {
    settings.relay.enabled = false;
    settings.relay.linked_peer_id = None;
    settings.relay.linked_peer_hostname = None;
    settings.relay.linked_peer_ip = None;
    settings.relay.remote_token = None;
    settings.relay.last_known_peer = None;
    settings
}

async fn request_relay_auth_token(
    client: &reqwest::Client,
    base_url: &str,
    origin: &str,
    app_name: &str,
) -> Result<RelayAuthTokenResponse, String> {
    let request_url = format!("{}/v1/auth/request", base_url.trim_end_matches('/'));
    let response = client
        .post(&request_url)
        .header(reqwest::header::ORIGIN, origin)
        .json(&serde_json::json!({ "appName": app_name }))
        .timeout(std::time::Duration::from_secs(
            crate::server::CONSENT_REQUEST_TIMEOUT_SECS + RELAY_LINK_REQUEST_TIMEOUT_GRACE_SECS,
        ))
        .send()
        .await
        .map_err(|error| format!("Failed to contact relay peer at {}: {}", base_url, error))?;

    if response.status().is_success() {
        return response
            .json::<RelayAuthTokenResponse>()
            .await
            .map_err(|error| format!("Relay peer returned an invalid auth response: {}", error));
    }

    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    let payload = serde_json::from_slice::<RelayApiErrorResponse>(&body).ok();

    let message = payload
        .and_then(|error| {
            error.error.or_else(|| {
                error
                    .code
                    .map(|code| format!("Relay link failed with {}.", code))
            })
        })
        .unwrap_or_else(|| {
            format!(
                "Relay peer rejected the link request with status {}.",
                status
            )
        });

    Err(message)
}

fn sync_settings_to_server_state(state: &AppState, settings: &Settings) {
    let server_settings = crate::server::ServerSettings::from(settings);
    if let Ok(mut current) = state.server_state.settings.try_write() {
        *current = server_settings;
    } else {
        let server_settings = server_settings.clone();
        let server_state = state.server_state.clone();
        tauri::async_runtime::spawn(async move {
            *server_state.settings.write().await = server_settings;
        });
    }
}

fn persist_settings_and_emit(
    state: &AppState,
    app: &AppHandle,
    settings: Settings,
) -> Result<Settings, String> {
    let updated = state.settings_manager.update(settings);
    sync_settings_to_server_state(state, &updated);
    app.emit("settings-sync", &updated)
        .map_err(|error| error.to_string())?;
    Ok(updated)
}

fn stream_settings_patch(settings: &serde_json::Value) -> serde_json::Value {
    let mut patch = serde_json::Map::new();
    let Some(object) = settings.as_object() else {
        return serde_json::Value::Object(patch);
    };

    for key in [
        "resolution",
        "quality",
        "framegen",
        "hdr",
        "executorPreference",
        "encodingProfiles",
    ] {
        if let Some(value) = object.get(key) {
            patch.insert(key.to_string(), value.clone());
        }
    }

    serde_json::Value::Object(patch)
}

async fn relay_api_error_message(response: reqwest::Response, fallback: &str) -> String {
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    let payload = serde_json::from_slice::<RelayApiErrorResponse>(&body).ok();

    payload
        .and_then(|error| {
            error.error.or_else(|| {
                error
                    .code
                    .map(|code| format!("Relay request failed with {}.", code))
            })
        })
        .unwrap_or_else(|| format!("{} (HTTP {}).", fallback, status))
}

async fn forward_relay_stream_settings(
    client: &reqwest::Client,
    settings: &Settings,
    patch: &serde_json::Value,
) -> Result<(), String> {
    if patch
        .as_object()
        .map(|object| object.is_empty())
        .unwrap_or(true)
    {
        return Ok(());
    }

    let saved_peer = saved_relay_peer_metadata(settings)
        .ok_or_else(|| "REFEREE Relay is enabled, but no linked peer is configured.".to_string())?;
    let ip = saved_peer.ip.clone().ok_or_else(|| {
        "REFEREE Relay is enabled, but the linked peer is missing an IP address.".to_string()
    })?;
    let remote_token = settings.relay.remote_token.clone().ok_or_else(|| {
        "REFEREE Relay is enabled, but no saved relay control token was found.".to_string()
    })?;

    let live_peer = probe_relay_peer(
        client,
        &ip,
        std::time::Duration::from_millis(RELAY_STATUS_TIMEOUT_MS),
    )
    .await?;

    if !relay_identity_matches_link(&saved_peer, &live_peer) {
        return Err(
            "A different REFEREE instance responded at the linked relay address.".to_string(),
        );
    }

    let response = client
        .post(format!("http://{}:14002/v1/settings/stream", ip))
        .header("X-Referee-Token", remote_token)
        .json(patch)
        .timeout(std::time::Duration::from_secs(RELAY_SETTINGS_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|error| {
            format!(
                "Failed to update stream settings on the linked REFEREE instance: {}",
                error
            )
        })?;

    if response.status().is_success() {
        return Ok(());
    }

    Err(relay_api_error_message(
        response,
        "Linked REFEREE instance rejected the stream settings update",
    )
    .await)
}

/// Scans the local /24 subnet for other REFEREE instances by probing `GET /v1/ping`
/// on port 14002 at each address. Returns all peers that respond within 300 ms.
#[tauri::command]
pub async fn discover_lan_peers(state: State<'_, AppState>) -> Result<Vec<RelayPeer>, String> {
    let local_ip = crate::server::local_ip();

    let octets = match local_ip {
        std::net::IpAddr::V4(v4) => v4.octets(),
        _ => return Ok(vec![]),
    };

    let subnet_prefix = format!("{}.{}.{}", octets[0], octets[1], octets[2]);
    let local_octet = octets[3];

    let client = state.http_client.clone();
    let local_instance_id = Some(state.server_state.instance_id.as_str());

    let tasks: Vec<_> = (1u8..=254)
        .filter(|&h| h != local_octet)
        .map(|host| {
            let ip = format!("{}.{}", subnet_prefix, host);
            let url = format!("http://{}:14002/v1/ping", ip);
            let client = client.clone();
            tokio::spawn(async move {
                let result = client
                    .get(&url)
                    .timeout(std::time::Duration::from_millis(RELAY_DISCOVERY_TIMEOUT_MS))
                    .send()
                    .await;

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.json::<RelayPingResponse>().await {
                            Ok(ping) => Some(relay_peer_from_ping(ip, ping)),
                            Err(_) => None,
                        }
                    }
                    _ => None,
                }
            })
        })
        .collect();

    let mut peers = Vec::new();
    for task in tasks {
        if let Ok(Some(peer)) = task.await {
            peers.push(peer);
        }
    }

    Ok(dedupe_and_filter_relay_peers(peers, local_instance_id))
}

#[tauri::command]
pub async fn get_linked_relay_status(
    state: State<'_, AppState>,
) -> Result<LinkedRelayStatus, String> {
    let settings = state.settings_manager.get();
    Ok(resolve_linked_relay_status(&state.http_client, &settings).await)
}

#[tauri::command]
pub async fn link_relay_peer(
    peer: RelayPeer,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if peer.instance_id.is_none() {
        return Err(
            "Selected relay peer did not report an instance ID. Update that REFEREE installation before linking."
                .to_string(),
        );
    }

    let origin = relay_origin_for_instance_id(&state.server_state.instance_id);
    let app_name = relay_link_app_name();
    let base_url = format!("http://{}:14002", peer.ip);
    let auth = request_relay_auth_token(&state.http_client, &base_url, &origin, &app_name).await?;

    if !auth.persistent {
        return Err(
            "Relay linking requires a persistent approval on the selected peer. Approve the request again and choose the saved option."
                .to_string(),
        );
    }

    let mut settings = state.settings_manager.get();
    settings.instance_id = Some(state.server_state.instance_id.clone());
    let settings = build_linked_relay_settings(settings, &peer, auth.token)?;
    let _ = persist_settings_and_emit(state.inner(), &app, settings)?;
    Ok(())
}

#[tauri::command]
pub async fn unlink_relay_peer(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let settings = clear_linked_relay_settings(state.settings_manager.get());
    let _ = persist_settings_and_emit(state.inner(), &app, settings)?;
    Ok(())
}

fn semver_gt(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> (u64, u64, u64) {
        let mut parts = v.splitn(3, '.');
        let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts
            .next()
            .and_then(|p| p.split('-').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        (major, minor, patch)
    };
    parse(a) > parse(b)
}

#[tauri::command]
pub async fn stop_stream(
    session_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    server::stop_tracked_session(&session_id, &state.server_state.sessions).await?;

    if state.server_state.sessions.is_empty() {
        if let Some(window) = app.get_webview_window(crate::tray::STREAM_PLAYER_WINDOW_LABEL) {
            let _ = window.destroy();
        }
    }

    let gpu = state.server_state.gpu_info.read().await.clone();
    let settings = state.server_state.settings.read().await.clone();
    let caps = state.server_state.encoder_capabilities.read().await.clone();
    let setup_complete = state
        .server_state
        .setup_complete
        .load(std::sync::atomic::Ordering::Acquire);
    let status = server::build_status(
        &gpu,
        &settings,
        &state.server_state.sessions,
        caps.as_ref(),
        setup_complete,
    );
    let status = server::apply_status_readiness(status, &gpu, &settings, setup_complete).await;
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn retry_setup(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state
        .setup_state
        .lock()
        .expect("setup state poisoned")
        .in_progress
    {
        return Ok(());
    }

    let lib_dir = state.lib_dir.clone();
    let gpu_info = state.server_state.gpu_info.read().await.clone();
    if !is_supported_setup_vendor(&gpu_info.vendor) {
        {
            let mut setup_state = state.setup_state.lock().expect("setup state poisoned");
            setup_state.has_started = true;
            setup_state.in_progress = false;
            setup_state.complete = true;
            setup_state.error = None;
            setup_state.progress = Some(deps::SetupProgress {
                phase: "done".to_string(),
                percent: 100,
                detail: "Relay mode ready".to_string(),
            });
        }
        state
            .server_state
            .setup_complete
            .store(true, std::sync::atomic::Ordering::Release);
        state.setup_done.notify_one();
        let _ = app.emit("setup-complete", ());
        return Ok(());
    }

    let server_state = state.server_state.clone();
    let setup_done = state.setup_done.clone();
    let setup_state = state.setup_state.clone();
    let http_client = state.http_client.clone();

    run_setup_download(
        app,
        server_state,
        lib_dir,
        setup_done,
        setup_state,
        gpu_info.vendor,
        gpu_info.name,
        http_client,
    )
    .await
}

// --- Fire-and-forget commands ---

#[tauri::command]
pub async fn save_settings(
    settings: serde_json::Value,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let updated = state.settings_manager.merge_and_update(settings);

    // Sync to server
    {
        let mut server_settings = state.server_state.settings.write().await;
        *server_settings = crate::server::ServerSettings::from(&updated);
    }

    // Apply window always-on-top
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(updated.always_on_top);
    }

    let _ = app.emit("settings-sync", &updated);
    Ok(())
}

#[tauri::command]
pub async fn save_stream_settings(
    settings: serde_json::Value,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let stream_patch = stream_settings_patch(&settings);
    let updated = state.settings_manager.merge_and_update(settings);

    sync_settings_to_server_state(state.inner(), &updated);

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_always_on_top(updated.always_on_top);
    }

    let _ = app.emit("settings-sync", &updated);

    if relay_link_is_configured(&updated) {
        forward_relay_stream_settings(&state.http_client, &updated, &stream_patch).await?;
    }

    Ok(())
}

#[tauri::command]
pub fn set_boot_setting(enable: bool, app: AppHandle) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    if enable {
        autolaunch.enable().map_err(|e| e.to_string())
    } else {
        autolaunch.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn open_external(url: String) {
    // Restrict to safe web schemes to prevent registered URI handlers (steam://, ms-word://, etc.)
    // from being triggered via injected content in the WebView.
    if url.starts_with("https://") || url.starts_with("http://") {
        let _ = open::that(&url);
    }
}

#[tauri::command]
pub fn open_github() {
    let _ = open::that("https://github.com/cmacrowther/referee");
}

#[tauri::command]
pub async fn minimize_window(window: Window, state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings_manager.get();
    if settings.minimize_to_tray {
        let _ = window.hide();
    } else {
        let _ = window.minimize();
    }
    Ok(())
}

#[tauri::command]
pub async fn close_window(
    window: Window,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let settings = state.settings_manager.get();
    if settings.close_to_tray {
        // Destroy the window so it is recreated fresh on next open.
        let _ = window.destroy();
    } else {
        crate::server::shutdown(&state.server_state.sessions).await;
        app.exit(0);
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_pin(
    enable: bool,
    window: Window,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let _ = window.set_always_on_top(enable);

    let mut settings = state.settings_manager.get();
    settings.always_on_top = enable;
    state.settings_manager.update(settings.clone());

    let _ = app.emit("settings-sync", &settings);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererReadyResponse {
    pub setup_needed: bool,
    pub gpu_vendor: String,
    pub gpu_name: Option<String>,
    pub setup_in_progress: bool,
    pub setup_complete: bool,
    pub setup_progress: Option<deps::SetupProgress>,
    pub setup_error: Option<String>,
}

/// Builds a snapshot of setup and GPU readiness for the renderer.
///
/// # Returns
/// A `RendererReadyResponse` containing whether binaries are needed, the detected GPU vendor and name,
/// and the current setup lifecycle fields (`in_progress`, `complete`, `progress`, `error`).
///
/// # Examples
///
async fn build_setup_response(state: &AppState) -> RendererReadyResponse {
    let lib_dir = state.lib_dir.clone();
    let gpu = state.server_state.gpu_info.read().await.clone();
    let setup_snapshot = state
        .setup_state
        .lock()
        .expect("setup state poisoned")
        .clone();

    RendererReadyResponse {
        setup_needed: is_supported_setup_vendor(&gpu.vendor)
            && !deps::binaries_ready(&lib_dir, &gpu.vendor),
        gpu_vendor: gpu.vendor,
        gpu_name: gpu.name,
        setup_in_progress: setup_snapshot.in_progress,
        setup_complete: setup_snapshot.complete,
        setup_progress: setup_snapshot.progress,
        setup_error: setup_snapshot.error,
    }
}

/// Creates the initial `deps::SetupProgress` for the encoder setup phase based on the GPU vendor.
///
/// The returned progress has `phase` set to `"encoder"`, `percent` set to `0`, and a vendor-specific
/// `detail` message describing which encoder binary will be prepared.
///
/// # Examples
///
fn initial_setup_progress(gpu_vendor: &str) -> deps::SetupProgress {
    let detail = match gpu_vendor {
        "nvidia" => "Preparing NVEncC download...",
        "amd" => "Preparing VCEEncC download...",
        _ => "Preparing required binaries...",
    };

    deps::SetupProgress {
        phase: "encoder".to_string(),
        percent: 0,
        detail: detail.to_string(),
    }
}

/// Downloads platform-specific encoder binaries for the given GPU, updates shared server state and setup lifecycle as progress occurs, and emits Tauri events for progress, completion, or error.
///
/// The function updates `setup_state` to reflect lifecycle changes, writes refreshed GPU and encoder capability information into `server_state` on success, notifies `setup_done` when finished (success or failure), and emits the following events on `app`:
/// - `"setup-gpu-detected"` when GPU/vendor are reported
/// - `"setup-progress"` for each progress update
/// - `"setup-complete"` on success
/// - `"setup-error"` on failure
///
/// # Parameters
/// - `setup_done`: notified once when the setup run finishes (success or failure).
/// - `setup_state`: mutex-protected `SetupLifecycle` that will be updated with progress, completion, and error details.
///
/// # Returns
/// `Ok(())` on successful download and post-download capability detection; `Err(String)` with an error message if the download or subsequent steps fail.
///
/// # Examples
///
async fn run_setup_download(
    app: AppHandle,
    server_state: ServerState,
    lib_dir: PathBuf,
    setup_done: Arc<Notify>,
    setup_state: Arc<Mutex<SetupLifecycle>>,
    gpu_vendor: String,
    gpu_name: Option<String>,
    http_client: reqwest::Client,
) -> Result<(), String> {
    let initial_progress = initial_setup_progress(&gpu_vendor);

    {
        let mut state = setup_state.lock().expect("setup state poisoned");
        state.has_started = true;
        state.in_progress = true;
        state.complete = false;
        state.progress = Some(initial_progress.clone());
        state.error = None;
    }

    let _ = app.emit(
        "setup-gpu-detected",
        serde_json::json!({ "vendor": gpu_vendor.clone(), "name": gpu_name.clone() }),
    );
    let _ = app.emit("setup-progress", &initial_progress);

    let progress_state = setup_state.clone();
    let progress_app = app.clone();
    let result = deps::download_binaries(&http_client, &lib_dir, &gpu_vendor, move |progress| {
        {
            let mut state = progress_state.lock().expect("setup state poisoned");
            state.progress = Some(progress.clone());
            state.complete = progress.phase == "done";
            state.error = None;
        }
        let _ = progress_app.emit("setup-progress", &progress);
    })
    .await;

    match result {
        Ok(()) => {
            let lib_dir_spawn = lib_dir.clone();
            let new_gpu = tokio::task::spawn_blocking(move || {
                crate::gpu::detect_compatible_gpu(&lib_dir_spawn)
            })
            .await
            .unwrap_or_default();
            info!("[Setup]: GPU refreshed after setup: {:?}", new_gpu);
            *server_state.gpu_info.write().await = new_gpu;

            // Detect and cache encoder capabilities now that the binary is present.
            let (backend, enc_path, rife_worker_path) = {
                let g = server_state.gpu_info.read().await;
                (
                    g.backend.clone(),
                    g.encoder_path.clone(),
                    g.rife_worker_path.clone(),
                )
            };
            if let (Some(backend), Some(enc_path)) = (backend, enc_path) {
                let rife = rife_worker_path
                    .as_deref()
                    .map(crate::pipeline::detect_rife_capability)
                    .unwrap_or(false);
                let caps = crate::pipeline::detect_encoder_capabilities(&enc_path, &backend)
                    .with_rife(rife);
                info!(
                    "[Setup]: Encoder capabilities — fruc={}, truehdr={}, rife={}",
                    caps.has_fruc, caps.has_truehdr, caps.has_rife
                );
                *server_state.encoder_capabilities.write().await = Some(caps);
            }

            {
                let mut state = setup_state.lock().expect("setup state poisoned");
                state.in_progress = false;
                state.complete = true;
                state.error = None;
                state.progress = Some(deps::SetupProgress {
                    phase: "done".to_string(),
                    percent: 100,
                    detail: "Setup complete".to_string(),
                });
            }

            setup_done.notify_one();
            server_state
                .setup_complete
                .store(true, std::sync::atomic::Ordering::Release);
            let _ = app.emit("setup-complete", ());
            Ok(())
        }
        Err(error) => {
            {
                let mut state = setup_state.lock().expect("setup state poisoned");
                state.in_progress = false;
                state.complete = false;
                state.error = Some(error.clone());
            }

            tracing::error!("[Setup]: Binary download failed: {}", error);
            setup_done.notify_one();
            let _ = app.emit(
                "setup-error",
                serde_json::json!({ "message": error.clone() }),
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn get_setup_state(state: State<'_, AppState>) -> Result<RendererReadyResponse, String> {
    Ok(build_setup_response(state.inner()).await)
}

/// Handle the renderer's "ready" signal and ensure setup is started when required.
///
/// This inspects whether required binaries are present for the detected GPU, updates the shared
/// setup lifecycle state, spawns the background setup task if setup is needed and hasn't started,
/// and returns a snapshot describing current setup and GPU readiness.
///
/// # Returns
///
/// `RendererReadyResponse` containing setup-needed flags, GPU vendor/name, lifecycle fields,
/// and any current setup progress or error.
///
/// # Examples
///
#[tauri::command]
pub async fn renderer_ready(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RendererReadyResponse, String> {
    info!("[Main]: Renderer signaled ready");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }

    let lib_dir = state.lib_dir.clone();
    let gpu = state.server_state.gpu_info.read().await.clone();
    let setup_needed =
        is_supported_setup_vendor(&gpu.vendor) && !deps::binaries_ready(&lib_dir, &gpu.vendor);
    let mut should_spawn_setup = false;
    let setup_snapshot = {
        let mut setup_state = state.setup_state.lock().expect("setup state poisoned");

        if setup_needed {
            if !setup_state.has_started && !setup_state.in_progress {
                setup_state.has_started = true;
                setup_state.in_progress = true;
                setup_state.complete = false;
                setup_state.error = None;
                setup_state.progress = Some(initial_setup_progress(&gpu.vendor));
                should_spawn_setup = true;
            }
        } else {
            setup_state.has_started = true;
            setup_state.in_progress = false;
            setup_state.complete = true;
            setup_state.error = None;
            setup_state.progress = Some(deps::SetupProgress {
                phase: "done".to_string(),
                percent: 100,
                detail: if is_supported_setup_vendor(&gpu.vendor) {
                    "Setup complete".to_string()
                } else {
                    "Relay mode ready".to_string()
                },
            });
        }

        setup_state.clone()
    };

    if setup_needed && should_spawn_setup {
        let app_clone = app.clone();
        let server_state = state.server_state.clone();
        let lib_dir = state.lib_dir.clone();
        let setup_done = state.setup_done.clone();
        let setup_state = state.setup_state.clone();
        let gpu_vendor = gpu.vendor.clone();
        let gpu_name = gpu.name.clone();
        let http_client = state.http_client.clone();

        tokio::spawn(async move {
            let _ = run_setup_download(
                app_clone,
                server_state,
                lib_dir,
                setup_done,
                setup_state,
                gpu_vendor,
                gpu_name,
                http_client,
            )
            .await;
        });
    } else if !setup_needed {
        state
            .server_state
            .setup_complete
            .store(true, std::sync::atomic::Ordering::Release);
        state.setup_done.notify_one();
    }

    let _ = (setup_needed, setup_snapshot);

    Ok(build_setup_response(state.inner()).await)
}

// ---------------------------------------------------------------------------
// Player detection & launch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DetectedPlayer {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub installed: bool,
}

const BUILTIN_PLAYER_ID: &str = "builtin";
const CUSTOM_PLAYER_ID: &str = "custom";

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlayerLaunchTarget {
    BuiltinWindow,
    ExternalExecutable(String),
}

struct KnownPlayer {
    id: &'static str,
    name: &'static str,
    candidates: &'static [&'static str],
}

#[cfg(target_os = "windows")]
static KNOWN_PLAYERS: &[KnownPlayer] = &[
    KnownPlayer {
        id: "vlc",
        name: "VLC Media Player",
        candidates: &[
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
        ],
    },
    KnownPlayer {
        id: "mpv",
        name: "MPV",
        candidates: &[
            r"C:\Program Files\mpv\mpv.exe",
            r"C:\Program Files (x86)\mpv\mpv.exe",
        ],
    },
    KnownPlayer {
        id: "mpc-hc",
        name: "MPC-HC",
        candidates: &[
            r"C:\Program Files\MPC-HC\mpc-hc64.exe",
            r"C:\Program Files (x86)\MPC-HC\mpc-hc.exe",
            r"C:\Program Files\MPC-BE\mpc-be64.exe",
            r"C:\Program Files (x86)\MPC-BE\mpc-be.exe",
        ],
    },
    KnownPlayer {
        id: "potplayer",
        name: "PotPlayer",
        candidates: &[
            r"C:\Program Files\DAUM\PotPlayer\PotPlayerMini64.exe",
            r"C:\Program Files (x86)\Daum\PotPlayer\PotPlayerMini.exe",
            r"C:\Program Files\PotPlayer\PotPlayerMini64.exe",
        ],
    },
];

#[cfg(target_os = "macos")]
static KNOWN_PLAYERS: &[KnownPlayer] = &[
    KnownPlayer {
        id: "vlc",
        name: "VLC Media Player",
        candidates: &["/Applications/VLC.app/Contents/MacOS/VLC"],
    },
    KnownPlayer {
        id: "mpv",
        name: "MPV",
        candidates: &[
            "/opt/homebrew/bin/mpv",
            "/usr/local/bin/mpv",
            "/usr/bin/mpv",
        ],
    },
    KnownPlayer {
        id: "iina",
        name: "IINA",
        candidates: &["/Applications/IINA.app/Contents/MacOS/IINA"],
    },
];

#[cfg(target_os = "linux")]
static KNOWN_PLAYERS: &[KnownPlayer] = &[
    KnownPlayer {
        id: "vlc",
        name: "VLC Media Player",
        candidates: &["/usr/bin/vlc", "/usr/local/bin/vlc", "/snap/bin/vlc"],
    },
    KnownPlayer {
        id: "mpv",
        name: "MPV",
        candidates: &["/usr/bin/mpv", "/usr/local/bin/mpv"],
    },
];

fn find_player_path(id: &str) -> Option<String> {
    for player in KNOWN_PLAYERS {
        if player.id == id {
            for candidate in player.candidates {
                if std::path::Path::new(candidate).exists() {
                    return Some(candidate.to_string());
                }
            }
            return None;
        }
    }
    None
}

fn resolve_player_launch_target(player: &PlayerSettings) -> Result<PlayerLaunchTarget, String> {
    if !player.enabled {
        return Err("Player auto-open is not enabled".to_string());
    }

    match player.selected_player.as_deref() {
        Some(BUILTIN_PLAYER_ID) => Ok(PlayerLaunchTarget::BuiltinWindow),
        Some(CUSTOM_PLAYER_ID) => player
            .custom_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(|path| PlayerLaunchTarget::ExternalExecutable(path.to_string()))
            .ok_or_else(|| "Custom player path is not set".to_string()),
        Some(id) => find_player_path(id)
            .map(PlayerLaunchTarget::ExternalExecutable)
            .ok_or_else(|| format!("Selected player '{}' could not be found on this system", id)),
        None => Err("No player selected".to_string()),
    }
}

#[tauri::command]
pub fn detect_players() -> Vec<DetectedPlayer> {
    let mut results = vec![DetectedPlayer {
        id: BUILTIN_PLAYER_ID.to_string(),
        name: "REFEREE Built-in Player".to_string(),
        path: None,
        installed: true,
    }];

    results.extend(KNOWN_PLAYERS.iter().map(|p| {
        let path = find_player_path(p.id);
        let installed = path.is_some();
        DetectedPlayer {
            id: p.id.to_string(),
            name: p.name.to_string(),
            path,
            installed,
        }
    }));

    // Always append the custom-player sentinel so the UI can render the entry.
    results.push(DetectedPlayer {
        id: CUSTOM_PLAYER_ID.to_string(),
        name: "Custom Player".to_string(),
        path: None,
        installed: false,
    });

    results
}

#[tauri::command]
pub async fn launch_player(
    url: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let settings = state.settings_manager.get();
    match resolve_player_launch_target(&settings.player)? {
        PlayerLaunchTarget::BuiltinWindow => {
            let window = crate::tray::get_or_create_stream_player_window(&app)
                .map_err(|e| format!("Failed to open built-in player: {}", e))?;
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            Ok(())
        }
        PlayerLaunchTarget::ExternalExecutable(exe_path) => {
            let mut command = std::process::Command::new(&exe_path);
            command.arg(&url);
            crate::process::hide_std_command_window(&mut command);
            command
                .spawn()
                .map_err(|e| format!("Failed to launch player: {}", e))?;

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_linked_relay_settings, clear_linked_relay_settings, dedupe_and_filter_relay_peers,
        probe_relay_peer_at_url, relay_identity_matches_link, relay_origin_for_instance_id,
        relay_peer_from_ping, request_relay_auth_token, resolve_player_launch_target,
        PlayerLaunchTarget, RelayPeer, RelayPingResponse, BUILTIN_PLAYER_ID, CUSTOM_PLAYER_ID,
        RELAY_LINK_REQUEST_TIMEOUT_GRACE_SECS,
    };
    use crate::settings::{PlayerSettings, Settings};
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    async fn spawn_auth_server(
        status: StatusCode,
        body: serde_json::Value,
        seen_origin: Option<Arc<Mutex<Option<String>>>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route(
                "/v1/auth/request",
                post(
                    move |headers: HeaderMap,
                          State((status, body, seen_origin)): State<(
                        StatusCode,
                        serde_json::Value,
                        Option<Arc<Mutex<Option<String>>>>,
                    )>| async move {
                        if let Some(slot) = seen_origin {
                            *slot.lock().unwrap() = headers
                                .get(reqwest::header::ORIGIN)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string);
                        }

                        (status, Json(body))
                    },
                ),
            )
            .with_state((status, body, seen_origin));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{}", address), handle)
    }

    async fn spawn_ping_server(body: serde_json::Value) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/v1/ping",
            get(move || {
                let body = body.clone();
                async move { Json(body) }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{}", address), handle)
    }

    #[test]
    fn resolve_player_launch_target_prefers_builtin_window() {
        let settings = PlayerSettings {
            enabled: true,
            selected_player: Some(BUILTIN_PLAYER_ID.to_string()),
            custom_path: None,
        };

        assert_eq!(
            resolve_player_launch_target(&settings),
            Ok(PlayerLaunchTarget::BuiltinWindow)
        );
    }

    #[test]
    fn resolve_player_launch_target_returns_trimmed_custom_path() {
        let settings = PlayerSettings {
            enabled: true,
            selected_player: Some(CUSTOM_PLAYER_ID.to_string()),
            custom_path: Some("  C:\\tools\\player.exe  ".to_string()),
        };

        assert_eq!(
            resolve_player_launch_target(&settings),
            Ok(PlayerLaunchTarget::ExternalExecutable(
                "C:\\tools\\player.exe".to_string()
            ))
        );
    }

    #[test]
    fn resolve_player_launch_target_requires_enabled_player_setting() {
        let settings = PlayerSettings {
            enabled: false,
            selected_player: Some(BUILTIN_PLAYER_ID.to_string()),
            custom_path: None,
        };

        assert_eq!(
            resolve_player_launch_target(&settings),
            Err("Player auto-open is not enabled".to_string())
        );
    }

    #[test]
    fn relay_peer_from_ping_preserves_extended_metadata() {
        let peer = relay_peer_from_ping(
            "192.168.1.20".to_string(),
            RelayPingResponse {
                instance_id: Some("peer-1".to_string()),
                hostname: Some(" media-box ".to_string()),
                version: Some(" 1.2.3 ".to_string()),
                platform: Some(" linux ".to_string()),
                gpu_ready: Some(true),
                gpu_vendor: Some(" nvidia ".to_string()),
                gpu_name: Some(" RTX 4080 ".to_string()),
            },
        );

        assert_eq!(peer.instance_id.as_deref(), Some("peer-1"));
        assert_eq!(peer.hostname, "media-box");
        assert_eq!(peer.version, "1.2.3");
        assert_eq!(peer.platform.as_deref(), Some("linux"));
        assert_eq!(peer.gpu_ready, Some(true));
        assert_eq!(peer.gpu_vendor.as_deref(), Some("nvidia"));
        assert_eq!(peer.gpu_name.as_deref(), Some("RTX 4080"));
    }

    #[test]
    fn dedupe_and_filter_relay_peers_filters_local_instance_and_duplicates() {
        let peers = vec![
            RelayPeer {
                instance_id: Some("peer-2".to_string()),
                ip: "192.168.1.30".to_string(),
                hostname: "peer-two".to_string(),
                version: "1.0.0".to_string(),
                platform: Some("linux".to_string()),
                gpu_ready: Some(true),
                gpu_vendor: Some("amd".to_string()),
                gpu_name: Some("RX 7900".to_string()),
            },
            RelayPeer {
                instance_id: Some("local-instance".to_string()),
                ip: "192.168.1.10".to_string(),
                hostname: "this-machine".to_string(),
                version: "1.0.0".to_string(),
                platform: Some("windows".to_string()),
                gpu_ready: Some(true),
                gpu_vendor: Some("nvidia".to_string()),
                gpu_name: Some("RTX 4080".to_string()),
            },
            RelayPeer {
                instance_id: Some("peer-2".to_string()),
                ip: "192.168.1.31".to_string(),
                hostname: "peer-two-duplicate".to_string(),
                version: "1.0.1".to_string(),
                platform: Some("linux".to_string()),
                gpu_ready: Some(false),
                gpu_vendor: Some("amd".to_string()),
                gpu_name: Some("RX 7900".to_string()),
            },
            RelayPeer {
                instance_id: None,
                ip: "192.168.1.25".to_string(),
                hostname: "legacy-peer".to_string(),
                version: "0.9.0".to_string(),
                platform: None,
                gpu_ready: None,
                gpu_vendor: None,
                gpu_name: None,
            },
        ];

        let filtered = dedupe_and_filter_relay_peers(peers, Some("local-instance"));

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].ip, "192.168.1.25");
        assert_eq!(filtered[1].ip, "192.168.1.30");
        assert_eq!(filtered[1].hostname, "peer-two");
    }

    #[test]
    fn relay_origin_for_instance_id_generates_stable_synthetic_origin() {
        assert_eq!(
            relay_origin_for_instance_id("  Peer_123  "),
            "https://peer-peer-123.referee.invalid"
        );
    }

    #[test]
    fn build_linked_relay_settings_persists_selected_peer_metadata() {
        let settings = build_linked_relay_settings(
            Settings::default(),
            &RelayPeer {
                instance_id: Some("peer-1".to_string()),
                ip: "192.168.1.25".to_string(),
                hostname: "media-box".to_string(),
                version: "1.2.3".to_string(),
                platform: Some("linux".to_string()),
                gpu_ready: Some(true),
                gpu_vendor: Some("nvidia".to_string()),
                gpu_name: Some("RTX 4080".to_string()),
            },
            "relay-secret".to_string(),
        )
        .unwrap();

        assert!(settings.relay.enabled);
        assert_eq!(settings.relay.linked_peer_id.as_deref(), Some("peer-1"));
        assert_eq!(
            settings.relay.linked_peer_hostname.as_deref(),
            Some("media-box")
        );
        assert_eq!(
            settings.relay.linked_peer_ip.as_deref(),
            Some("192.168.1.25")
        );
        assert_eq!(settings.relay.remote_token.as_deref(), Some("relay-secret"));
        assert_eq!(
            settings
                .relay
                .last_known_peer
                .as_ref()
                .and_then(|peer| peer.gpu_name.as_deref()),
            Some("RTX 4080")
        );
    }

    #[test]
    fn clear_linked_relay_settings_removes_saved_credentials_and_selection() {
        let settings = clear_linked_relay_settings(
            build_linked_relay_settings(
                Settings::default(),
                &RelayPeer {
                    instance_id: Some("peer-1".to_string()),
                    ip: "192.168.1.25".to_string(),
                    hostname: "media-box".to_string(),
                    version: "1.2.3".to_string(),
                    platform: Some("linux".to_string()),
                    gpu_ready: Some(true),
                    gpu_vendor: Some("nvidia".to_string()),
                    gpu_name: Some("RTX 4080".to_string()),
                },
                "relay-secret".to_string(),
            )
            .unwrap(),
        );

        assert!(!settings.relay.enabled);
        assert!(settings.relay.linked_peer_id.is_none());
        assert!(settings.relay.linked_peer_hostname.is_none());
        assert!(settings.relay.linked_peer_ip.is_none());
        assert!(settings.relay.remote_token.is_none());
        assert!(settings.relay.last_known_peer.is_none());
    }

    #[test]
    fn relay_link_request_timeout_outlives_remote_consent_timeout() {
        assert_eq!(
            crate::server::CONSENT_REQUEST_TIMEOUT_SECS + RELAY_LINK_REQUEST_TIMEOUT_GRACE_SECS,
            195
        );
    }

    #[tokio::test]
    async fn request_relay_auth_token_returns_token_and_forwards_origin() {
        let seen_origin = Arc::new(Mutex::new(None));
        let (base_url, handle) = spawn_auth_server(
            StatusCode::OK,
            json!({
                "token": "relay-secret",
                "persistent": true
            }),
            Some(seen_origin.clone()),
        )
        .await;

        let client = reqwest::Client::new();
        let response = request_relay_auth_token(
            &client,
            &base_url,
            "https://peer-local-instance.referee.invalid",
            "REFEREE Relay",
        )
        .await
        .unwrap();

        assert_eq!(response.token, "relay-secret");
        assert!(response.persistent);
        assert_eq!(
            seen_origin.lock().unwrap().as_deref(),
            Some("https://peer-local-instance.referee.invalid")
        );

        handle.abort();
    }

    #[tokio::test]
    async fn request_relay_auth_token_surfaces_remote_error_messages() {
        let (base_url, handle) = spawn_auth_server(
            StatusCode::FORBIDDEN,
            json!({
                "code": "NO_APP_HANDLE",
                "error": "Desktop app handle is unavailable."
            }),
            None,
        )
        .await;

        let client = reqwest::Client::new();
        let error = request_relay_auth_token(
            &client,
            &base_url,
            "https://peer-local-instance.referee.invalid",
            "REFEREE Relay",
        )
        .await
        .unwrap_err();

        assert_eq!(error, "Desktop app handle is unavailable.");

        handle.abort();
    }

    #[tokio::test]
    async fn probe_relay_peer_at_url_parses_ping_payload() {
        let (base_url, handle) = spawn_ping_server(json!({
            "instanceId": "peer-1",
            "hostname": "media-box",
            "version": "1.2.3",
            "platform": "linux",
            "gpuReady": true,
            "gpuVendor": "nvidia",
            "gpuName": "RTX 4080"
        }))
        .await;

        let client = reqwest::Client::new();
        let peer = probe_relay_peer_at_url(
            &client,
            &format!("{}/v1/ping", base_url),
            "127.0.0.1",
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();

        assert_eq!(peer.instance_id.as_deref(), Some("peer-1"));
        assert_eq!(peer.hostname, "media-box");
        assert_eq!(peer.version, "1.2.3");
        assert_eq!(peer.platform.as_deref(), Some("linux"));
        assert_eq!(peer.gpu_ready, Some(true));
        assert_eq!(peer.gpu_vendor.as_deref(), Some("nvidia"));
        assert_eq!(peer.gpu_name.as_deref(), Some("RTX 4080"));

        handle.abort();
    }

    #[test]
    fn relay_identity_matches_link_requires_same_instance_id() {
        let saved = crate::settings::RelayPeerMetadata {
            instance_id: Some("peer-1".to_string()),
            hostname: Some("media-box".to_string()),
            ip: Some("192.168.1.25".to_string()),
            version: None,
            platform: None,
            gpu_ready: None,
            gpu_vendor: None,
            gpu_name: None,
        };
        let live = RelayPeer {
            instance_id: Some("peer-2".to_string()),
            ip: "192.168.1.25".to_string(),
            hostname: "other-box".to_string(),
            version: "1.2.3".to_string(),
            platform: Some("linux".to_string()),
            gpu_ready: Some(true),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_name: Some("RTX 4080".to_string()),
        };

        assert!(!relay_identity_matches_link(&saved, &live));
    }
}
