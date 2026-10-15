use std::sync::LazyLock;

use super::{RELAY_CONNECT_TIMEOUT_MS, RELAY_TMP_PROXY_READ_TIMEOUT_SECS};

static REMOTE_TMP_PROXY_HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> =
    LazyLock::new(build_remote_tmp_proxy_http_client);

static RELAY_CONTROL_HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> =
    LazyLock::new(build_relay_control_http_client);

pub(super) fn build_remote_tmp_proxy_http_client() -> Result<reqwest::Client, String> {
    // The read timeout bounds a stalled peer mid-segment, so the player's own
    // retry logic takes over instead of the proxied request hanging forever.
    reqwest::Client::builder()
        .user_agent("REFEREE-RelayTmpProxy")
        .connect_timeout(std::time::Duration::from_millis(RELAY_CONNECT_TIMEOUT_MS))
        .read_timeout(std::time::Duration::from_secs(
            RELAY_TMP_PROXY_READ_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|error| format!("Failed to build remote tmp proxy HTTP client: {}", error))
}

pub(super) fn build_relay_control_http_client() -> Result<reqwest::Client, String> {
    // Peers sit on the same LAN, so a connect that has not completed quickly
    // means the peer is offline. Failing fast keeps stop, keepalive, and status
    // calls snappy instead of each waiting out its full request timeout.
    reqwest::Client::builder()
        .user_agent("REFEREE-RelayControl")
        .connect_timeout(std::time::Duration::from_millis(RELAY_CONNECT_TIMEOUT_MS))
        .build()
        .map_err(|error| format!("Failed to build relay control HTTP client: {}", error))
}

pub(super) fn remote_tmp_proxy_http_client() -> Result<&'static reqwest::Client, String> {
    REMOTE_TMP_PROXY_HTTP_CLIENT
        .as_ref()
        .map_err(std::clone::Clone::clone)
}

pub(super) fn relay_control_http_client() -> Result<&'static reqwest::Client, String> {
    RELAY_CONTROL_HTTP_CLIENT
        .as_ref()
        .map_err(std::clone::Clone::clone)
}

pub(crate) fn validate_http_clients() -> Result<(), String> {
    remote_tmp_proxy_http_client()?;
    relay_control_http_client()?;
    Ok(())
}
