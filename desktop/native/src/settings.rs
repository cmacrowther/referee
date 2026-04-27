use crate::pipeline::{EncodingProfile, ExecutorPreference, MAX_VSR_QUALITY};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::RwLock;
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSettings {
    pub enabled: bool,
    #[serde(rename = "selectedPlayer")]
    pub selected_player: Option<String>,
    #[serde(rename = "customPath")]
    pub custom_path: Option<String>,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            selected_player: None,
            custom_path: None,
        }
    }
}

fn normalize_optional_string(value: &mut Option<String>) {
    *value = value.take().and_then(|text| {
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelayPeerMetadata {
    #[serde(
        rename = "instanceId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(rename = "gpuReady", default, skip_serializing_if = "Option::is_none")]
    pub gpu_ready: Option<bool>,
    #[serde(rename = "gpuVendor", default, skip_serializing_if = "Option::is_none")]
    pub gpu_vendor: Option<String>,
    #[serde(rename = "gpuName", default, skip_serializing_if = "Option::is_none")]
    pub gpu_name: Option<String>,
}

impl RelayPeerMetadata {
    fn normalize(&mut self) {
        normalize_optional_string(&mut self.instance_id);
        normalize_optional_string(&mut self.hostname);
        normalize_optional_string(&mut self.ip);
        normalize_optional_string(&mut self.version);
        normalize_optional_string(&mut self.platform);
        normalize_optional_string(&mut self.gpu_vendor);
        normalize_optional_string(&mut self.gpu_name);
    }

    fn is_empty(&self) -> bool {
        self.instance_id.is_none()
            && self.hostname.is_none()
            && self.ip.is_none()
            && self.version.is_none()
            && self.platform.is_none()
            && self.gpu_ready.is_none()
            && self.gpu_vendor.is_none()
            && self.gpu_name.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelaySettings {
    pub enabled: bool,
    #[serde(
        rename = "linkedPeerId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub linked_peer_id: Option<String>,
    #[serde(
        rename = "linkedPeerHostname",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub linked_peer_hostname: Option<String>,
    #[serde(
        rename = "linkedPeerIp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub linked_peer_ip: Option<String>,
    #[serde(
        rename = "remoteToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub remote_token: Option<String>,
    #[serde(
        rename = "lastKnownPeer",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_known_peer: Option<RelayPeerMetadata>,
}

impl RelaySettings {
    fn normalize(&mut self) {
        normalize_optional_string(&mut self.linked_peer_id);
        normalize_optional_string(&mut self.linked_peer_hostname);
        normalize_optional_string(&mut self.linked_peer_ip);
        normalize_optional_string(&mut self.remote_token);

        if let Some(peer) = &mut self.last_known_peer {
            peer.normalize();
            if peer.is_empty() {
                self.last_known_peer = None;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub resolution: String,
    pub quality: u8,
    pub framegen: bool,
    pub hdr: bool,
    #[serde(rename = "executorPreference", default)]
    pub executor_preference: ExecutorPreference,
    #[serde(rename = "alwaysOnTop")]
    pub always_on_top: bool,
    #[serde(rename = "showOnProxyStart")]
    pub show_on_proxy_start: bool,
    #[serde(rename = "minimizeToTray")]
    pub minimize_to_tray: bool,
    #[serde(rename = "closeToTray")]
    pub close_to_tray: bool,
    pub notifications: bool,
    #[serde(rename = "encodingProfiles", default)]
    pub encoding_profiles: HashMap<String, EncodingProfile>,
    #[serde(default)]
    pub player: PlayerSettings,
    #[serde(
        rename = "instanceId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub relay: RelaySettings,
    #[serde(rename = "apiToken", default, skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            resolution: "1920x1080".to_string(),
            quality: 2,
            framegen: true,
            hdr: true,
            executor_preference: ExecutorPreference::Auto,
            always_on_top: false,
            show_on_proxy_start: false,
            minimize_to_tray: false,
            close_to_tray: true,
            notifications: false,
            encoding_profiles: HashMap::new(),
            player: PlayerSettings::default(),
            instance_id: None,
            relay: RelaySettings::default(),
            api_token: None,
        }
    }
}

impl Settings {
    pub fn normalize(&mut self) {
        let valid_resolutions = ["1920x1080", "2560x1440", "3840x2160"];
        if !valid_resolutions.contains(&self.resolution.as_str()) {
            self.resolution = "1920x1080".to_string();
        }
        self.quality = self.quality.clamp(1, MAX_VSR_QUALITY);
        normalize_optional_string(&mut self.instance_id);
        self.relay.normalize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedOriginMeta {
    #[serde(rename = "appName")]
    pub app_name: Option<String>,
    #[serde(rename = "approvedAt")]
    pub approved_at: String,
}

pub struct SettingsManager {
    path: std::path::PathBuf,
    settings: RwLock<Settings>,
}

impl SettingsManager {
    pub fn new(config_path: &Path) -> Self {
        let settings = Self::load_from_disk(config_path);
        Self {
            path: config_path.to_path_buf(),
            settings: RwLock::new(settings),
        }
    }

    /// Loads settings from the JSON file at the given path, returning a normalized `Settings` or defaults on failure.
    ///
    /// Attempts to read and deserialize the file if it exists. On successful parse the returned `Settings` will have
    /// `normalize()` applied. If the file does not exist or reading/parsing fails, this returns `Settings::default()`
    /// and logs an error.
    ///
    /// # Parameters
    ///
    /// - `config_path`: Path to the settings JSON file.
    ///
    /// # Returns
    ///
    /// `Settings` loaded and normalized from `config_path`, or `Settings::default()` if the file is missing or cannot be read/parsed.
    ///
    /// # Examples
    ///
    fn load_from_disk(config_path: &Path) -> Settings {
        if config_path.exists() {
            match fs::read_to_string(config_path) {
                Ok(data) => match serde_json::from_str::<Settings>(&data) {
                    Ok(mut s) => {
                        s.normalize();
                        return s;
                    }
                    Err(e) => error!("[Settings]: Failed to parse settings: {}", e),
                },
                Err(e) => error!("[Settings]: Failed to read settings file: {}", e),
            }
        }
        Settings::default()
    }

    pub fn get(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Update the manager's settings: normalize the provided settings, record top-level changes to the log when detectable, replace the in-memory settings, persist them to disk, and return the applied settings.
    ///
    /// This call normalizes `new_settings` before applying it. When possible, it computes a top-level JSON diff against the previous settings and logs each changed key. The in-memory settings are replaced atomically under a write lock; the settings are then saved to disk (save failures are logged but do not affect the returned value).
    ///
    /// # Returns
    ///
    /// The normalized `Settings` value that was stored.
    ///
    /// # Examples
    ///
    pub fn update(&self, new_settings: Settings) -> Settings {
        let mut s = new_settings;
        s.normalize();
        // Acquire write lock so read/compare/update are atomic across threads.
        let mut guard = self.settings.write().unwrap();
        let previous = guard.clone();

        // Determine which top-level settings changed by comparing JSON values.
        match (serde_json::to_value(&previous), serde_json::to_value(&s)) {
            (Ok(prev_v), Ok(new_v)) => {
                if let (Some(prev_map), Some(new_map)) = (prev_v.as_object(), new_v.as_object()) {
                    for (k, new_val) in new_map.iter() {
                        let prev_val = prev_map.get(k);
                        if prev_val != Some(new_val) {
                            info!("[Settings]: Setting changed: {} = {}", k, new_val);
                        }
                    }
                }
            }
            _ => info!("[Settings]: Settings updated (could not compute diff)"),
        }

        // Update in-memory settings while still holding the lock, then release.
        *guard = s.clone();
        drop(guard);

        if let Err(e) = self.save_to_disk(&s) {
            error!("[Settings]: Failed to save settings: {}", e);
        }

        s
    }

    pub fn merge_and_update(&self, partial: serde_json::Value) -> Settings {
        // Helpers to reduce repetition when extracting scalar fields from a JSON object.
        macro_rules! apply_str {
            ($obj:expr, $key:literal, $dst:expr) => {
                if let Some(v) = $obj.get($key).and_then(|v| v.as_str()) {
                    $dst = v.to_string();
                }
            };
        }
        macro_rules! apply_opt_str {
            ($obj:expr, $key:literal, $dst:expr) => {
                if $obj.contains_key($key) {
                    $dst = $obj.get($key).and_then(|v| v.as_str()).map(String::from);
                }
            };
        }
        macro_rules! apply_bool {
            ($obj:expr, $key:literal, $dst:expr) => {
                if let Some(v) = $obj.get($key).and_then(|v| v.as_bool()) {
                    $dst = v;
                }
            };
        }
        macro_rules! apply_u8 {
            ($obj:expr, $key:literal, $dst:expr) => {
                if let Some(v) = $obj.get($key).and_then(|v| v.as_u64()) {
                    $dst = v as u8;
                }
            };
        }

        let mut current = self.get();
        if let Some(obj) = partial.as_object() {
            apply_str!(obj, "resolution", current.resolution);
            apply_u8!(obj, "quality", current.quality);
            apply_bool!(obj, "framegen", current.framegen);
            apply_bool!(obj, "hdr", current.hdr);

            if let Some(v) = obj.get("executorPreference") {
                if let Ok(preference) = serde_json::from_value::<ExecutorPreference>(v.clone()) {
                    current.executor_preference = preference;
                }
            }

            apply_bool!(obj, "alwaysOnTop", current.always_on_top);
            apply_bool!(obj, "showOnProxyStart", current.show_on_proxy_start);
            apply_bool!(obj, "minimizeToTray", current.minimize_to_tray);
            apply_bool!(obj, "closeToTray", current.close_to_tray);
            apply_bool!(obj, "notifications", current.notifications);

            if let Some(profiles_val) = obj.get("encodingProfiles") {
                if let Ok(profiles) =
                    serde_json::from_value::<HashMap<String, EncodingProfile>>(profiles_val.clone())
                {
                    current.encoding_profiles = profiles;
                }
            }

            if let Some(player_val) = obj.get("player") {
                if let Some(player_obj) = player_val.as_object() {
                    apply_bool!(player_obj, "enabled", current.player.enabled);
                    if player_obj.contains_key("selectedPlayer") {
                        current.player.selected_player = player_obj
                            .get("selectedPlayer")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                    if player_obj.contains_key("customPath") {
                        current.player.custom_path = player_obj
                            .get("customPath")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                }
            }

            if let Some(instance_id) = obj.get("instanceId").and_then(|v| v.as_str()) {
                current.instance_id = Some(instance_id.to_string());
            }

            if let Some(relay_val) = obj.get("relay") {
                if relay_val.is_null() {
                    current.relay = RelaySettings::default();
                } else if let Some(relay_obj) = relay_val.as_object() {
                    apply_bool!(relay_obj, "enabled", current.relay.enabled);
                    apply_opt_str!(relay_obj, "linkedPeerId", current.relay.linked_peer_id);
                    apply_opt_str!(
                        relay_obj,
                        "linkedPeerHostname",
                        current.relay.linked_peer_hostname
                    );
                    apply_opt_str!(relay_obj, "linkedPeerIp", current.relay.linked_peer_ip);
                    apply_opt_str!(relay_obj, "remoteToken", current.relay.remote_token);

                    if relay_obj.contains_key("lastKnownPeer") {
                        current.relay.last_known_peer =
                            relay_obj.get("lastKnownPeer").and_then(|value| {
                                if value.is_null() {
                                    None
                                } else {
                                    serde_json::from_value::<RelayPeerMetadata>(value.clone()).ok()
                                }
                            });
                    }
                }
            }
        }
        self.update(current)
    }

    fn save_to_disk(&self, settings: &Settings) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(&self.path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::{ApprovedOriginMeta, RelayPeerMetadata, RelaySettings, Settings, SettingsManager};

    #[test]
    fn approved_origin_meta_serializes_with_camel_case_keys() {
        let meta = ApprovedOriginMeta {
            app_name: Some("My App".to_string()),
            approved_at: "1700000000".to_string(),
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert!(json.get("appName").is_some(), "expected appName key");
        assert!(json.get("approvedAt").is_some(), "expected approvedAt key");
        assert!(
            json.get("app_name").is_none(),
            "should not have snake_case app_name"
        );
        assert!(
            json.get("approved_at").is_none(),
            "should not have snake_case approved_at"
        );
        assert_eq!(json["appName"], "My App");
        assert_eq!(json["approvedAt"], "1700000000");
    }

    #[test]
    fn settings_api_token_round_trips_through_json() {
        let mut s = Settings::default();
        s.api_token = Some("my-secret-token".to_string());
        let json = serde_json::to_string(&s).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.api_token.as_deref(), Some("my-secret-token"));
    }

    #[test]
    fn settings_relay_fields_round_trip_through_json() {
        let mut s = Settings::default();
        s.instance_id = Some("instance-123".to_string());
        s.relay = RelaySettings {
            enabled: true,
            linked_peer_id: Some("peer-1".to_string()),
            linked_peer_hostname: Some("media-box".to_string()),
            linked_peer_ip: Some("192.168.1.23".to_string()),
            remote_token: Some("relay-secret".to_string()),
            last_known_peer: Some(RelayPeerMetadata {
                instance_id: Some("peer-1".to_string()),
                hostname: Some("media-box".to_string()),
                ip: Some("192.168.1.23".to_string()),
                version: Some("1.2.3".to_string()),
                platform: Some("linux".to_string()),
                gpu_ready: Some(true),
                gpu_vendor: Some("nvidia".to_string()),
                gpu_name: Some("RTX 4080".to_string()),
            }),
        };

        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["instanceId"], "instance-123");
        assert_eq!(json["relay"]["linkedPeerId"], "peer-1");
        assert_eq!(json["relay"]["lastKnownPeer"]["gpuVendor"], "nvidia");

        let parsed: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.instance_id.as_deref(), Some("instance-123"));
        assert!(parsed.relay.enabled);
        assert_eq!(parsed.relay.linked_peer_id.as_deref(), Some("peer-1"));
        assert_eq!(
            parsed
                .relay
                .last_known_peer
                .as_ref()
                .and_then(|peer| peer.gpu_name.as_deref()),
            Some("RTX 4080")
        );
    }

    #[test]
    fn settings_api_token_none_is_omitted_from_serialized_json() {
        let s = Settings::default();
        assert!(s.api_token.is_none());
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("apiToken"),
            "apiToken field should be absent when None"
        );
    }

    #[test]
    fn settings_manager_on_nonexistent_path_has_no_api_token() {
        let path = std::env::temp_dir().join(format!("referee-test-{}.json", uuid::Uuid::new_v4()));
        let manager = SettingsManager::new(&path);
        assert!(manager.get().api_token.is_none());
    }

    #[test]
    fn settings_manager_merge_and_update_applies_relay_fields() {
        let path = std::env::temp_dir().join(format!("referee-test-{}.json", uuid::Uuid::new_v4()));
        let manager = SettingsManager::new(&path);

        let updated = manager.merge_and_update(serde_json::json!({
            "instanceId": "instance-123",
            "relay": {
                "enabled": true,
                "linkedPeerId": "peer-1",
                "linkedPeerHostname": "media-box",
                "linkedPeerIp": "192.168.1.23",
                "remoteToken": "relay-secret",
                "lastKnownPeer": {
                    "instanceId": "peer-1",
                    "hostname": "media-box",
                    "ip": "192.168.1.23",
                    "version": "1.2.3",
                    "platform": "linux",
                    "gpuReady": true,
                    "gpuVendor": "nvidia",
                    "gpuName": "RTX 4080"
                }
            }
        }));

        assert_eq!(updated.instance_id.as_deref(), Some("instance-123"));
        assert!(updated.relay.enabled);
        assert_eq!(updated.relay.linked_peer_id.as_deref(), Some("peer-1"));
        assert_eq!(
            updated
                .relay
                .last_known_peer
                .as_ref()
                .and_then(|peer| peer.version.as_deref()),
            Some("1.2.3")
        );
        assert_eq!(
            updated
                .relay
                .last_known_peer
                .as_ref()
                .and_then(|peer| peer.platform.as_deref()),
            Some("linux")
        );
    }
}
