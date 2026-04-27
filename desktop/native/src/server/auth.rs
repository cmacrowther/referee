use super::*;

#[derive(Deserialize)]
pub(super) struct AuthRequestBody {
    app_name: Option<String>,
}

#[derive(Serialize)]
pub(super) struct AuthTokenResponse {
    token: String,
    /// `true` when the approval is persisted (already-approved origin or "Always Allow");
    /// `false` when the user chose "Allow Once" and the grant is session-only.
    persistent: bool,
}

/// Persists the approved-origins map to a JSON file on disk.
pub fn persist_approved_origins(
    origins: &DashMap<String, ApprovedOriginMeta>,
    path: &std::path::Path,
) {
    let map: HashMap<String, ApprovedOriginMeta> = origins
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();
    match serde_json::to_string_pretty(&map) {
        Ok(json) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(path, json) {
                error!("[Auth]: Failed to persist approved origins: {}", e);
            }
        }
        Err(e) => error!("[Auth]: Failed to serialize approved origins: {}", e),
    }
}

/// Loads the approved-origins map from a JSON file. Returns an empty map if the file is
/// absent or cannot be parsed (errors are logged but not propagated).
pub fn load_approved_origins(path: &std::path::Path) -> DashMap<String, ApprovedOriginMeta> {
    let map = DashMap::new();
    if !path.exists() {
        return map;
    }
    match std::fs::read_to_string(path) {
        Ok(data) => {
            if let Ok(parsed) = serde_json::from_str::<HashMap<String, ApprovedOriginMeta>>(&data) {
                for (k, v) in parsed {
                    map.insert(k, v);
                }
            }
        }
        Err(e) => error!("[Auth]: Failed to read approved origins: {}", e),
    }
    map
}

/// Handles `POST /v1/auth/request`.
///
/// A web page posts its `appName` here (the body is display-only). The browser
/// automatically supplies the `Origin` header which reflects the actual page origin
/// and cannot be forged by page-level JavaScript.
///
/// - If the origin is already in the approved-origins list the token is returned
///   immediately without prompting the user.
/// - On the desktop app a native consent dialog is shown; the user can approve once
///   or choose "Always Allow" (which persists the approval).
/// - In headless mode (no desktop UI) the endpoint always returns 403 so callers
///   fall back to the manual X-Referee-Token header workflow.
pub(super) async fn handle_auth_request(
    State(state): State<ServerState>,
    RemoteIp(ip_str): RemoteIp,
    request: axum::http::Request<axum::body::Body>,
) -> axum::response::Response {
    // Rate limit: 5 requests per minute per IP to prevent consent-spam from LAN neighbors.
    let rate_key = format!("auth:{}", ip_str);
    if !state.rate_limit_auth.check(&rate_key) {
        return json_error_response_with_headers(
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, "60")],
            error_codes::RATE_LIMITED,
            "Too many authorization requests. Please wait before retrying.",
        );
    }

    // Extract origin before consuming the body — browsers set this on all
    // cross-origin requests and it cannot be forged by page-level JavaScript.
    let origin = request
        .headers()
        .get("Origin")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body_bytes = axum::body::to_bytes(request.into_body(), 8192)
        .await
        .unwrap_or_default();

    let Some(origin) = origin else {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            error_codes::MISSING_ORIGIN,
            "Origin header is required.",
        );
    };

    if !origin.starts_with("http://") && !origin.starts_with("https://") {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_ORIGIN,
            "Origin must be a valid http or https URL.",
        );
    }

    let app_name: Option<String> = serde_json::from_slice::<AuthRequestBody>(&body_bytes)
        .ok()
        .and_then(|b| b.app_name)
        .filter(|s| !s.trim().is_empty());

    // Already approved — return token without interrupting the user.
    if state.approved_origins.contains_key(&origin) {
        let token = state.api_token.read().unwrap().clone();
        return Json(AuthTokenResponse {
            token,
            persistent: true,
        })
        .into_response();
    }

    request_user_consent(state, origin, app_name).await
}

/// In headless mode there is no desktop UI available to show a consent prompt.
#[cfg(feature = "headless")]
pub(super) async fn request_user_consent(
    _state: ServerState,
    origin: String,
    _app_name: Option<String>,
) -> axum::response::Response {
    json_error_response(
        StatusCode::FORBIDDEN,
        error_codes::HEADLESS_MODE,
        format!(
            "User consent is not available in headless mode. \
             To allow access from '{}', either: \
             (1) set REFEREE_ALLOWED_ORIGINS to include this origin before starting the server, \
             or (2) add it via POST /v1/origins with a valid X-Referee-Token header, \
             or (3) supply the X-Referee-Token header directly on protected requests.",
            origin
        ),
    )
}

/// Brings the REFEREE window to the front, emits a `consent-request` event to the
/// desktop renderer, and waits up to 3 minutes for the user's decision.
#[cfg(not(feature = "headless"))]
pub(super) async fn request_user_consent(
    state: ServerState,
    origin: String,
    app_name: Option<String>,
) -> axum::response::Response {
    use tauri::Emitter;

    let Some(app_handle) = state.app_handle.clone() else {
        return json_error_response(
            StatusCode::FORBIDDEN,
            error_codes::NO_APP_HANDLE,
            "Desktop app handle is unavailable.",
        );
    };

    let nonce = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<ConsentDecision>();
    state.pending_consents.insert(nonce.clone(), tx);

    let payload = serde_json::json!({
        "nonce": nonce,
        "origin": origin,
        "appName": app_name,
    });

    // Store the payload so the desktop UI can retrieve it even if it missed
    // the Tauri event (e.g. window was freshly created after the emit, or the
    // user manually brings REFEREE to the front).
    *state.pending_consent_ui.lock().unwrap() = Some(payload.clone());

    // Raise the window BEFORE emitting the event. Read whether the window was
    // already pinned on top so we can restore it after the prompt is dismissed.
    // Temporarily forcing always-on-top bypasses Windows' focus-stealing
    // prevention so the consent modal is visible above the browser even when
    // REFEREE does not hold the system foreground right.
    let was_always_on_top = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let was_always_on_top_writer = was_always_on_top.clone();
    let _ = app_handle.run_on_main_thread({
        let app = app_handle.clone();
        let window_payload = payload.clone();
        move || {
            if let Ok(window) = crate::tray::get_or_create_window(&app) {
                was_always_on_top_writer.store(
                    window.is_always_on_top().unwrap_or(false),
                    std::sync::atomic::Ordering::Relaxed,
                );
                let _ = window.set_always_on_top(true);
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                let _ = window.emit("consent-request", &window_payload);
            }
        }
    });

    // Emit after queuing the window raise. Delayed retries cover the case where
    // a hidden or freshly-created WebView misses the first event while it is
    // being shown; the renderer also polls pending_consent_ui as a fallback.
    let _ = app_handle.emit("consent-request", &payload);
    {
        let app = app_handle.clone();
        let pending_consent_ui = state.pending_consent_ui.clone();
        let retry_payload = payload.clone();
        let retry_nonce = nonce.clone();
        tauri::async_runtime::spawn(async move {
            for delay_ms in [250_u64, 750, 1_500] {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                let still_pending = pending_consent_ui
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(|pending| pending.get("nonce"))
                    .and_then(|pending_nonce| pending_nonce.as_str())
                    == Some(retry_nonce.as_str());
                if !still_pending {
                    break;
                }
                let _ = app.emit("consent-request", &retry_payload);
                if let Some(window) = app.get_webview_window(crate::tray::MAIN_WINDOW_LABEL) {
                    let _ = window.emit("consent-request", &retry_payload);
                }
            }
        });
    }

    let decision = tokio::time::timeout(
        std::time::Duration::from_secs(CONSENT_REQUEST_TIMEOUT_SECS),
        rx,
    )
    .await;

    // Clear the stored payload regardless of outcome.
    *state.pending_consent_ui.lock().unwrap() = None;

    // Clean up — remove the pending entry whether or not the command handler
    // already consumed it (a no-op if it was already removed).
    state.pending_consents.remove(&nonce);

    // Restore always-on-top to the user's original preference now that the
    // consent prompt no longer needs to be forcibly visible.
    if !was_always_on_top.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = app_handle.run_on_main_thread({
            let app = app_handle.clone();
            move || {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_always_on_top(false);
                }
            }
        });
    }

    match decision {
        Ok(Ok(ConsentDecision {
            approved: true,
            always_allow,
        })) => {
            if always_allow {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default();
                state.approved_origins.insert(
                    origin.clone(),
                    ApprovedOriginMeta {
                        app_name: app_name.clone(),
                        approved_at: ts,
                    },
                );
                if let Some(ref path) = state.approved_origins_path {
                    persist_approved_origins(&state.approved_origins, path);
                }
            }
            Json(AuthTokenResponse {
                token: state.api_token.read().unwrap().clone(),
                persistent: always_allow,
            })
            .into_response()
        }
        Ok(Ok(ConsentDecision {
            approved: false, ..
        }))
        | Ok(Err(_)) => json_error_response(
            StatusCode::FORBIDDEN,
            error_codes::CONSENT_DENIED,
            "User denied the authorization request.",
        ),
        Err(_) => json_error_response(
            StatusCode::REQUEST_TIMEOUT,
            error_codes::CONSENT_TIMEOUT,
            "Authorization request timed out.",
        ),
    }
}

// ── Token auth middleware ────────────────────────────────────────────────────

/// Rejects requests that do not supply the correct `X-Referee-Token` header.
///
/// Applied to all mutating routes (`/v1/stream/start`, `/v1/stream/heartbeat/{session_id}`,
/// `/v1/stream/stop`). Read-only and streaming routes are intentionally left unauthenticated.
pub(super) async fn require_api_token(
    State(state): State<ServerState>,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let authorized = {
        let stored = state.api_token.read().unwrap();
        let header_token = request
            .headers()
            .get("X-Referee-Token")
            .and_then(|v| v.to_str().ok());
        header_token == Some(stored.as_str())
    };
    if !authorized {
        return json_error_response(
            StatusCode::UNAUTHORIZED,
            error_codes::UNAUTHORIZED,
            "Invalid or missing API token.",
        );
    }
    next.run(request).await
}

// ── /v1/auth/rotate-token ────────────────────────────────────────────────────

/// Generates a new API token, persists it to settings, and returns it.
///
/// Protected by the current `X-Referee-Token` header via `require_api_token` middleware.
/// The new token takes effect immediately — all subsequent `require_api_token` checks
/// will read the updated value from the shared `Arc<RwLock<String>>`.
pub(super) async fn handle_rotate_token(
    State(state): State<ServerState>,
) -> axum::response::Response {
    let new_token = uuid::Uuid::new_v4().to_string();

    // Update the shared RwLock in-place. All future require_api_token checks on any
    // cloned ServerState will read the new value because they all share the same Arc.
    *state.api_token.write().unwrap() = new_token.clone();

    // Persist so the new token survives a server restart.
    if let Some(ref path) = state.approved_origins_path {
        if let Some(parent) = path.parent() {
            let config_path = parent.join("config.json");
            let settings_manager = crate::settings::SettingsManager::new(&config_path);
            let mut s = settings_manager.get();
            s.api_token = Some(new_token.clone());
            settings_manager.update(s);
        }
    }

    // Notify the desktop UI so it can refresh its displayed token.
    #[cfg(not(feature = "headless"))]
    if let Some(ref app_handle) = state.app_handle {
        use tauri::Emitter;
        let _ = app_handle.emit("token-rotated", &new_token);
    }

    info!("[Auth]: API token rotated.");
    Json(serde_json::json!({ "token": new_token })).into_response()
}

pub(super) fn apply_stream_settings_patch(
    settings: &mut ServerSettings,
    patch: &serde_json::Value,
) {
    let Some(object) = patch.as_object() else {
        return;
    };

    if let Some(resolution) = object.get("resolution").and_then(|value| value.as_str()) {
        if matches!(resolution, "1920x1080" | "2560x1440" | "3840x2160") {
            settings.resolution = resolution.to_string();
        }
    }

    if let Some(quality) = object.get("quality").and_then(|value| value.as_u64()) {
        settings.quality = quality.clamp(1, crate::pipeline::MAX_VSR_QUALITY as u64) as u8;
    }

    if let Some(framegen) = object.get("framegen").and_then(|value| value.as_bool()) {
        settings.framegen = framegen;
    }

    if let Some(hdr) = object.get("hdr").and_then(|value| value.as_bool()) {
        settings.hdr = hdr;
    }

    if let Some(value) = object.get("executorPreference") {
        if let Ok(preference) = serde_json::from_value::<ExecutorPreference>(value.clone()) {
            settings.executor_preference = preference;
        }
    }

    if let Some(value) = object.get("encodingProfiles") {
        if let Ok(profiles) =
            serde_json::from_value::<HashMap<String, EncodingProfile>>(value.clone())
        {
            settings.encoding_profiles = profiles;
        }
    }
}

// ── /v1/settings/stream ────────────────────────────────────────────────────

pub(super) async fn handle_stream_settings_update(
    State(state): State<ServerState>,
    Json(patch): Json<serde_json::Value>,
) -> axum::response::Response {
    if !patch.is_object() {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_SETTINGS,
            "Settings update must be a JSON object.",
        );
    }

    if let Some(settings_manager) = state.settings_manager.as_ref() {
        let updated = settings_manager.merge_and_update(patch);

        {
            let mut current = state.settings.write().await;
            *current = ServerSettings::from(&updated);
        }

        #[cfg(not(feature = "headless"))]
        if let Some(ref app_handle) = state.app_handle {
            let _ = app_handle.emit("settings-sync", &updated);
        }
    } else if let Some(path) = state.settings_path.as_ref() {
        let settings_manager = SettingsManager::new(path);
        let updated = settings_manager.merge_and_update(patch);

        {
            let mut current = state.settings.write().await;
            *current = ServerSettings::from(&updated);
        }

        #[cfg(not(feature = "headless"))]
        if let Some(ref app_handle) = state.app_handle {
            let _ = app_handle.emit("settings-sync", &updated);
        }
    } else {
        let mut current = state.settings.write().await;
        apply_stream_settings_patch(&mut current, &patch);
    }

    Json(OkResponse {
        status: "ok".to_string(),
    })
    .into_response()
}

// ── /v1/origins ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(super) struct OriginEntry {
    origin: String,
    #[serde(rename = "appName")]
    app_name: Option<String>,
    #[serde(rename = "approvedAt")]
    approved_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AddOriginRequest {
    origin: String,
    app_name: Option<String>,
}

/// `GET /v1/origins` — returns all persistently approved origins.
pub(super) async fn handle_list_origins(
    State(state): State<ServerState>,
) -> axum::response::Response {
    let mut entries: Vec<OriginEntry> = state
        .approved_origins
        .iter()
        .map(|e| OriginEntry {
            origin: e.key().clone(),
            app_name: e.value().app_name.clone(),
            approved_at: e.value().approved_at.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.origin.cmp(&b.origin));
    Json(entries).into_response()
}

/// `POST /v1/origins` — adds or updates an approved origin.
pub(super) async fn handle_add_origin(
    State(state): State<ServerState>,
    Json(body): Json<AddOriginRequest>,
) -> axum::response::Response {
    let origin = body.origin.trim().to_string();
    if !origin.starts_with("http://") && !origin.starts_with("https://") {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_ORIGIN,
            "Origin must be a valid http or https URL.",
        );
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();

    state.approved_origins.insert(
        origin.clone(),
        ApprovedOriginMeta {
            app_name: body.app_name.clone(),
            approved_at: ts,
        },
    );

    if let Some(ref path) = state.approved_origins_path {
        persist_approved_origins(&state.approved_origins, path);
    }

    info!("[Auth]: Origin added via API: {}", origin);
    (
        StatusCode::CREATED,
        Json(OkResponse {
            status: "created".to_string(),
        }),
    )
        .into_response()
}

/// `DELETE /v1/origins/:origin` — removes an approved origin (URL-encoded).
pub(super) async fn handle_delete_origin(
    State(state): State<ServerState>,
    AxumPath(origin): AxumPath<String>,
) -> axum::response::Response {
    if state.approved_origins.remove(&origin).is_none() {
        return json_error_response(
            StatusCode::NOT_FOUND,
            error_codes::ORIGIN_NOT_FOUND,
            format!("Origin '{}' is not in the approved list.", origin),
        );
    }

    if let Some(ref path) = state.approved_origins_path {
        persist_approved_origins(&state.approved_origins, path);
    }

    info!("[Auth]: Origin removed via API: {}", origin);
    Json(OkResponse {
        status: "deleted".to_string(),
    })
    .into_response()
}
