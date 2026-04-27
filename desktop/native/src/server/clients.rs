use std::sync::LazyLock;

static REMOTE_TMP_PROXY_HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> =
    LazyLock::new(build_remote_tmp_proxy_http_client);

static RELAY_CONTROL_HTTP_CLIENT: LazyLock<Result<reqwest::Client, String>> =
    LazyLock::new(build_relay_control_http_client);

pub(super) fn build_remote_tmp_proxy_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("REFEREE-RelayTmpProxy")
        .build()
        .map_err(|error| format!("Failed to build remote tmp proxy HTTP client: {}", error))
}

pub(super) fn build_relay_control_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("REFEREE-RelayControl")
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
