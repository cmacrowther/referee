use super::*;

pub(super) async fn add_security_headers(
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::HeaderName::from_static("x-content-type-options"),
        axum::http::header::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::header::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("referrer-policy"),
        axum::http::header::HeaderValue::from_static("no-referrer"),
    );
    response
}

/// Start and run the HTTP server exposing status, stream control, input proxy, and temporary file endpoints.
///
/// Binds to 0.0.0.0:PORT, registers versioned routes under `/v1/` for `/v1/status`,
/// `/v1/stream/start`, `/v1/stream/heartbeat/{session_id}`, `/v1/stream/stop`,
/// `/v1/input/{session_id}`, and `/v1/tmp/{session_id}/{filename}`. Mutating stream
/// routes require a valid `X-Referee-Token` header. Applies a permissive CORS layer.
/// On bind failure due to the address being in use, logs an error and exits the process
/// with code 1; other bind failures are also logged and cause the process to exit.
/// The function runs the server until it is stopped.
///
/// # Examples
///
pub async fn start_server(state: ServerState) {
    // Clone the approved-origins map so the CORS predicate closure can reference it
    // without holding a borrow on `state`.  The Arc means both share the live map.
    let approved_origins_for_cors = state.approved_origins.clone();

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, parts| {
            // These endpoints must be reachable by any origin: /v1/auth/request so
            // that clients can initiate the approval flow before they are approved,
            // and /v1/status so that any page can detect whether REFEREE is running
            // (read-only, no sensitive data).
            if matches!(parts.uri.path(), "/v1/auth/request" | "/v1/status") {
                return true;
            }
            // All other endpoints are restricted to origins that the user has
            // explicitly approved via the consent flow or REFEREE_ALLOWED_ORIGINS.
            origin
                .to_str()
                .map(|s| approved_origins_for_cors.contains_key(s))
                .unwrap_or(false)
        }))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-referee-token"),
        ]);

    let protected_routes = Router::new()
        .route("/v1/stream/start", post(handle_stream_start))
        .route("/v1/stream/heartbeat/{session_id}", post(handle_heartbeat))
        .route("/v1/stream/stop", post(handle_stream_stop))
        .route("/v1/settings/stream", post(handle_stream_settings_update))
        .route("/v1/auth/rotate-token", post(handle_rotate_token))
        .route("/v1/origins", get(handle_list_origins))
        .route("/v1/origins", post(handle_add_origin))
        .route(
            "/v1/origins/{origin}",
            axum::routing::delete(handle_delete_origin),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_api_token,
        ));

    let app = Router::new()
        .route("/v1/status", get(handle_status))
        .route("/v1/ping", get(handle_ping))
        .route("/v1/auth/request", post(handle_auth_request))
        .route("/v1/input/{session_id}", get(handle_input_proxy))
        .route("/v1/tmp/{session_id}/{filename}", get(serve_tmp_file))
        .merge(protected_routes)
        // CORS must be the inner layer so its headers are present when the outer
        // security-headers middleware inspects the response.
        .layer(cors)
        .layer(axum::middleware::from_fn(add_security_headers))
        .with_state(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], PORT));
    info!("[Main]: REFEREE server listening on {}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            error!(
                concat!(
                    "[Main]: Port {} is already in use. Another instance of Referee may ",
                    "still be running. Please close it and try again."
                ),
                PORT
            );
            std::process::exit(1);
        }
        Err(e) => {
            error!("[Main]: Failed to bind server on {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server exited unexpectedly");
}

pub async fn shutdown(sessions: &SessionMap) {
    let ids: Vec<String> = sessions.iter().map(|e| e.key().clone()).collect();
    for id in ids {
        if let Err(error) = stop_tracked_session(&id, sessions).await {
            warn!(
                "[Relay]: Failed to stop session {} during shutdown cleanly: {}",
                id, error
            );
            cleanup_session(&id, sessions).await;
        }
    }
}
