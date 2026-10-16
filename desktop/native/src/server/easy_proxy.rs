//! EasyProxy routing as the server sees it: the route and extracted source a
//! stream start uses, and the lifecycle of the managed server (install at
//! startup, stop when idle, react to settings changes).

use super::*;
use crate::source::easy_proxy::ExtractedSource;

const IDLE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

fn managed_enabled(settings: &EasyProxySettings) -> bool {
    settings.enabled && settings.mode == EasyProxyMode::Managed
}

/// The EasyProxy route for a local stream start, or `None` when it is off.
///
/// A misconfigured, stopped, or still-installing server fails the start with
/// its own error rather than a pipeline timeout. The source is never fetched
/// directly instead: a user who turned EasyProxy on may depend on its
/// network path.
pub(super) async fn resolve_easy_proxy_route(
    service: &Arc<EasyProxyService>,
    settings: &EasyProxySettings,
) -> HandlerResult<Option<EasyProxyRoute>> {
    let unavailable = |message: String| {
        warn!("[EasyProxy]: {}", message);
        error_response(
            StatusCode::BAD_GATEWAY,
            error_codes::EASYPROXY_UNAVAILABLE,
            message,
        )
    };

    if !settings.enabled {
        return Ok(None);
    }
    let route = match settings.mode {
        EasyProxyMode::Managed => service.ensure_running().await.map_err(unavailable)?,
        EasyProxyMode::External => {
            let route =
                EasyProxyRoute::new(settings.url.as_deref(), settings.api_password.as_deref())
                    .map_err(unavailable)?;
            route.check().await.map_err(unavailable)?;
            route
        }
    };
    Ok(Some(route))
}

/// The source of a local stream start as EasyProxy's extractor resolves it
/// (`EasyProxyRoute::extract`). A URL it finds no video at fails the start
/// with EasyProxy's reason rather than a pipeline timeout.
pub(super) async fn extract_easy_proxy_source(
    route: &EasyProxyRoute,
    source_url: &str,
    headers: &HashMap<String, String>,
) -> HandlerResult<ExtractedSource> {
    route.extract(source_url, headers).await.map_err(|message| {
        warn!("[EasyProxy]: {}", message);
        error_response(
            StatusCode::BAD_GATEWAY,
            error_codes::EASYPROXY_EXTRACTION_FAILED,
            message,
        )
    })
}

/// Starts a missing or outdated install when managed EasyProxy is turned on,
/// then stops the server whenever no stream has used it for
/// `easy_proxy_service::IDLE_SHUTDOWN`.
pub(super) fn spawn_easy_proxy_supervisor(state: ServerState) {
    tokio::spawn(async move {
        let settings = state.settings.read().await.easy_proxy.clone();
        if managed_enabled(&settings) {
            start_install_if_needed(&state.easy_proxy);
        }

        let mut interval = tokio::time::interval(IDLE_CHECK_INTERVAL);
        loop {
            interval.tick().await;
            let in_use = easy_proxy_in_use(&state);
            state
                .easy_proxy
                .stop_if_idle(in_use, crate::easy_proxy_service::IDLE_SHUTDOWN)
                .await;
        }
    });
}

/// Applies a change to the EasyProxy settings: turning managed mode on
/// installs it if needed, and turning it off stops a server no stream uses.
pub(crate) async fn apply_easy_proxy_settings(
    state: &ServerState,
    previous: &EasyProxySettings,
    updated: &EasyProxySettings,
) {
    if previous == updated {
        return;
    }
    if managed_enabled(updated) {
        start_install_if_needed(&state.easy_proxy);
    } else {
        let in_use = easy_proxy_in_use(state);
        state
            .easy_proxy
            .stop_if_idle(in_use, std::time::Duration::ZERO)
            .await;
    }
}

fn start_install_if_needed(service: &Arc<EasyProxyService>) {
    use crate::deps::easy_proxy::InstallState;
    if matches!(
        service.install_state(),
        InstallState::Missing | InstallState::Outdated
    ) {
        service.start_install();
    }
}

fn easy_proxy_in_use(state: &ServerState) -> bool {
    state.sessions.iter().any(|entry| {
        entry
            .value()
            .easy_proxy()
            .is_some_and(|route| state.easy_proxy.serves(route))
    })
}
