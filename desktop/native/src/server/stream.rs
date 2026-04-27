use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StreamStartRequest {
    pub(super) url: String,
    #[serde(rename = "appName")]
    pub(super) app_name: Option<String>,
    #[serde(rename = "streamTitle")]
    pub(super) stream_title: Option<String>,
    pub(super) headers: Option<HashMap<String, String>>,
    #[serde(rename = "contentKind")]
    pub(super) content_kind: Option<SourceContentKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StreamStartResponse {
    #[serde(rename = "sessionId")]
    pub(super) session_id: String,
    pub(super) url: String,
    pub(super) resolution: String,
    #[serde(rename = "sourceResolution")]
    pub(super) source_resolution: Option<String>,
    #[serde(rename = "appName")]
    pub(super) app_name: Option<String>,
    #[serde(rename = "streamTitle")]
    pub(super) stream_title: Option<String>,
    #[serde(rename = "effectiveQuality")]
    pub(super) effective_quality: u8,
    #[serde(rename = "evictedSessions")]
    pub(super) evicted_sessions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StopRequest {
    #[serde(rename = "sessionId")]
    pub(super) session_id: Option<String>,
    #[serde(rename = "stopAll", default)]
    pub(super) stop_all: bool,
}

#[derive(Deserialize)]
pub(super) struct InputProxyRequest {
    url: String,
}

#[derive(Deserialize, Default)]
pub(super) struct TmpFileRequest {
    url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub(super) struct TmpFileResponseHeaders {
    pub(super) content_type: &'static str,
    pub(super) cache_control: &'static str,
    pub(super) legacy_no_cache: bool,
}

pub(super) fn tmp_file_response_headers(filename: &str) -> TmpFileResponseHeaders {
    if filename.ends_with(".m3u8") {
        TmpFileResponseHeaders {
            content_type: "application/vnd.apple.mpegurl",
            cache_control: LIVE_PLAYLIST_CACHE_CONTROL,
            legacy_no_cache: true,
        }
    } else if filename.ends_with(".ts") {
        TmpFileResponseHeaders {
            content_type: "video/mp2t",
            cache_control: HLS_SEGMENT_CACHE_CONTROL,
            legacy_no_cache: false,
        }
    } else {
        TmpFileResponseHeaders {
            content_type: "application/octet-stream",
            cache_control: "no-store",
            legacy_no_cache: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RuntimeBackend {
    pub(super) gpu_vendor: String,
    pub(super) encoder_backend: String,
    pub(super) encoder_path: PathBuf,
    pub(super) ffmpeg_path: PathBuf,
    pub(super) rife_worker_path: Option<PathBuf>,
    pub(super) rife_model_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(super) struct StartupContext {
    pub(super) runtime_backend: RuntimeBackend,
    pub(super) source_descriptor: SourceDescriptor,
    pub(super) pipeline_request: PipelineRequest,
    pub(super) planner_capabilities: BackendCapabilities,
    pub(super) execution_plan: ExecutionPlan,
    pub(super) quality_level: u8,
    pub(super) framegen_enabled: bool,
    pub(super) target_fps: Option<f64>,
}

pub(super) fn validate_stream_start_request(req: &StreamStartRequest) -> HandlerResult<()> {
    if req.url.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_REQUEST,
            "Missing stream URL",
        ));
    }

    // Only http and https stream sources are supported. file:// URLs and raw
    // filesystem paths would expose arbitrary local media files to callers.
    match reqwest::Url::parse(&req.url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {}
        Ok(_) => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_URL,
                "Only http and https stream URLs are supported.",
            ));
        }
        Err(_) => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_URL,
                "Invalid stream URL.",
            ));
        }
    }

    Ok(())
}

/// Validates client-supplied headers that will be forwarded to upstream HTTP requests.
///
/// Rejects headers that could hijack routing, manipulate proxies, or enable HTTP
/// header injection via embedded CRLF sequences.
pub(super) fn validate_forwarded_headers(headers: &HashMap<String, String>) -> HandlerResult<()> {
    const BLOCKED_HEADERS: &[&str] = &[
        "host",
        "x-forwarded-for",
        "x-forwarded-host",
        "x-forwarded-proto",
        "x-real-ip",
        "via",
        "forwarded",
        "transfer-encoding",
        "connection",
        "upgrade",
        "keep-alive",
        "proxy-connection",
        "te",
        "trailer",
    ];

    for (name, value) in headers {
        if BLOCKED_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_HEADERS,
                "A disallowed forwarded header name was supplied.",
            ));
        }
        if value.contains('\r') || value.contains('\n') {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_HEADERS,
                "Header values must not contain carriage return or newline characters.",
            ));
        }
    }

    Ok(())
}

/// Builds a RuntimeBackend from the provided GPU information.
///
/// Returns a service-unavailable error when no supported encoder is present.
/// Uses `"ffmpeg"` as the ffmpeg path when the GPU record does not provide one.
///
/// # Returns
///
/// `Ok(RuntimeBackend)` when an encoder backend and encoder path are available.
///
/// # Examples
///
pub(super) fn resolve_runtime_backend(gpu: &GpuInfo) -> HandlerResult<RuntimeBackend> {
    if !is_supported_local_gpu_vendor(&gpu.vendor) {
        return Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error_codes::RELAY_REQUIRED,
            "No supported local AMD or NVIDIA GPU was detected. Configure REFEREE Relay \
             to route streams through another REFEREE instance.",
        ));
    }

    match (gpu.backend.clone(), gpu.encoder_path.clone()) {
        (Some(encoder_backend), Some(encoder_path)) => Ok(RuntimeBackend {
            gpu_vendor: gpu.vendor.clone(),
            encoder_backend,
            encoder_path,
            ffmpeg_path: gpu
                .ffmpeg_path
                .clone()
                .unwrap_or_else(|| PathBuf::from("ffmpeg")),
            rife_worker_path: gpu.rife_worker_path.clone(),
            rife_model_path: gpu.rife_model_path.clone(),
        }),
        _ => Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error_codes::NO_ENCODER,
            "No supported encoder detected.",
        )),
    }
}

pub(super) fn source_dimensions(source_descriptor: &SourceDescriptor) -> Option<(u32, u32)> {
    let metadata = source_descriptor.metadata.as_ref()?;
    metadata.width.zip(metadata.height).or_else(|| {
        metadata
            .source_resolution
            .as_deref()
            .and_then(parse_resolution)
    })
}

pub(super) fn build_upscale_request(
    source_descriptor: &SourceDescriptor,
    output_resolution: &str,
    quality_level: u8,
) -> UpscaleRequest {
    let Some((output_width, output_height)) = parse_resolution(output_resolution) else {
        return UpscaleRequest::Off;
    };
    let Some((source_width, source_height)) = source_dimensions(source_descriptor) else {
        return UpscaleRequest::Off;
    };

    if output_width > source_width || output_height > source_height {
        UpscaleRequest::Quality(quality_level)
    } else {
        UpscaleRequest::Off
    }
}

#[cfg(test)]
pub(super) fn build_pipeline_request(
    settings: &ServerSettings,
    source_descriptor: &SourceDescriptor,
    quality_level: u8,
) -> PipelineRequest {
    build_pipeline_request_from_parts(
        &settings.resolution,
        settings.framegen,
        settings.hdr,
        source_descriptor,
        quality_level,
    )
}

pub(super) fn build_pipeline_request_from_parts(
    resolution: &str,
    framegen: bool,
    hdr: bool,
    source_descriptor: &SourceDescriptor,
    quality_level: u8,
) -> PipelineRequest {
    PipelineRequest {
        source_transport: source_descriptor.classification.transport,
        source_kind: source_descriptor.classification.kind,
        source_content_kind: source_descriptor
            .metadata
            .as_ref()
            .map(|metadata| metadata.content_kind)
            .unwrap_or(SourceContentKind::Unknown),
        source_resolution: source_descriptor
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.source_resolution.clone()),
        output_resolution: resolution.to_string(),
        source_fps: source_descriptor
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.source_fps),
        latency_mode: LatencyMode::Low,
        upscale: build_upscale_request(source_descriptor, resolution, quality_level),
        interpolation: if framegen {
            InterpolationRequest::To60
        } else {
            InterpolationRequest::Off
        },
        hdr: if hdr {
            HdrRequest::TonemapToHdr10
        } else {
            HdrRequest::Off
        },
    }
}

pub(super) fn active_upscaler_label(execution_plan: &ExecutionPlan) -> Option<String> {
    execution_plan
        .video_ops
        .iter()
        .find_map(|video_op| match video_op {
            VideoOp::Anime4k2xUpscale(_) => Some("Anime4K2x".to_string()),
            VideoOp::Artcnn2xUpscale(_) => Some("ArtCNN".to_string()),
            VideoOp::Resize(_) if execution_plan.executor == ExecutorKind::Universal => {
                Some("Libplacebo".to_string())
            }
            VideoOp::Resize(_) if execution_plan.executor == ExecutorKind::NvidiaSpecialized => {
                Some("NVIDIA AI".to_string())
            }
            _ => None,
        })
}

pub(super) fn log_source_content_decision(
    session_id: &str,
    source_descriptor: &SourceDescriptor,
    execution_plan: &ExecutionPlan,
) {
    let (content_kind, confidence) = source_descriptor
        .metadata
        .as_ref()
        .map(|metadata| (metadata.content_kind, metadata.content_kind_confidence))
        .unwrap_or((SourceContentKind::Unknown, None));
    let upscaler = active_upscaler_label(execution_plan).unwrap_or_else(|| "None".to_string());

    match content_kind {
        SourceContentKind::Animated | SourceContentKind::LiveAction => {
            let confidence_suffix = confidence
                .map(|value| format!(" ({:.0}% confidence)", value * 100.0))
                .unwrap_or_default();
            info!(
                "[Planner]: Session {} classified source as {}{}; selected upscaler {}.",
                session_id,
                content_kind.log_label(),
                confidence_suffix,
                upscaler
            );
        }
        SourceContentKind::Unknown => {
            let confidence_suffix = confidence
                .map(|value| format!(" (closest confidence {:.0}%)", value * 100.0))
                .unwrap_or_default();
            info!(
                "[Planner]: Session {} could not confidently classify source content{}; selected upscaler {}.",
                session_id,
                confidence_suffix,
                upscaler
            );
        }
    }
}

pub(super) fn detected_source_content_kind(
    source_descriptor: &SourceDescriptor,
) -> Option<SourceContentKind> {
    let content_kind = source_descriptor
        .metadata
        .as_ref()
        .map(|metadata| metadata.content_kind)
        .unwrap_or(SourceContentKind::Unknown);

    (!matches!(content_kind, SourceContentKind::Unknown)).then_some(content_kind)
}

/// Produces backend-specific encoder capabilities for a given pipeline request and executor preference.
///
/// This returns the `BackendCapabilities` adapted to the current runtime platform, GPU vendor,
/// encoder backend, the provided `pipeline_request`, and `executor_preference`.
///
/// # Examples
///
pub(super) async fn build_planner_capabilities(
    state: &ServerState,
    runtime_backend: &RuntimeBackend,
    pipeline_request: &PipelineRequest,
    executor_preference: ExecutorPreference,
) -> BackendCapabilities {
    let runtime_capabilities = state
        .encoder_capabilities
        .read()
        .await
        .clone()
        .unwrap_or_else(|| {
            pipeline::detect_encoder_capabilities(
                &runtime_backend.encoder_path,
                &runtime_backend.encoder_backend,
            )
        });

    runtime_capabilities.to_backend_capabilities_for_request(
        crate::exec::RuntimePlatform::current(),
        &runtime_backend.gpu_vendor,
        &runtime_backend.encoder_backend,
        pipeline_request,
        executor_preference,
    )
}

pub(super) fn build_execution_plan(
    pipeline_request: &PipelineRequest,
    planner_capabilities: &BackendCapabilities,
) -> ExecutionPlan {
    GraphPlanner::new().plan(pipeline_request, planner_capabilities)
}

pub(super) fn disable_universal_portable_interpolation(
    runtime_backend: &RuntimeBackend,
    execution_plan: &mut ExecutionPlan,
) {
    if execution_plan.executor != ExecutorKind::Universal
        || runtime_backend
            .gpu_vendor
            .trim()
            .eq_ignore_ascii_case("amd")
    {
        return;
    }

    for video_op in &mut execution_plan.video_ops {
        let VideoOp::Interpolate(interpolation) = video_op else {
            continue;
        };

        if interpolation.decision.realization == InterpolationRealization::PortableFallback {
            interpolation.decision = InterpolationDecision::disabled(
                InterpolationUnsupportedReason::PortableFallbackNotImplemented,
            );
        }
    }
}

pub(super) fn assemble_startup_context(
    runtime_backend: RuntimeBackend,
    source_descriptor: SourceDescriptor,
    pipeline_request: PipelineRequest,
    planner_capabilities: BackendCapabilities,
    quality_level: u8,
) -> StartupContext {
    let source_fps = pipeline_request.source_fps;
    let mut execution_plan = build_execution_plan(&pipeline_request, &planner_capabilities);
    disable_universal_portable_interpolation(&runtime_backend, &mut execution_plan);
    let framegen_enabled = pipeline::is_interpolation_enabled(&execution_plan);
    let target_fps = pipeline::planned_target_frame_rate(source_fps, &execution_plan);

    StartupContext {
        runtime_backend,
        source_descriptor,
        pipeline_request,
        planner_capabilities,
        execution_plan,
        quality_level,
        framegen_enabled,
        target_fps,
    }
}

pub(super) async fn build_startup_context(
    state: &ServerState,
    settings: &LocalStreamSettings,
    runtime_backend: &RuntimeBackend,
    session_id: &str,
    source_url: &str,
    headers: &HashMap<String, String>,
    quality_level: u8,
    content_kind_override: Option<SourceContentKind>,
) -> StartupContext {
    let ffprobe_path = {
        let gpu = state.gpu_info.read().await;
        gpu.ffmpeg_path.as_deref().map(|p| {
            let ffprobe_name = if cfg!(target_os = "windows") {
                "ffprobe.exe"
            } else {
                "ffprobe"
            };
            p.with_file_name(ffprobe_name)
        })
    };
    let mut source_descriptor = describe_source(
        INPUT_PROXY_HOST,
        PORT,
        session_id,
        source_url,
        headers,
        ffprobe_path.as_deref(),
    )
    .await;

    // Apply explicit content kind override, bypassing content detection.
    if let Some(kind) = content_kind_override {
        if let Some(ref mut metadata) = source_descriptor.metadata {
            metadata.content_kind = kind;
            metadata.content_kind_confidence = Some(1.0);
        } else {
            source_descriptor.metadata = Some(SourceMetadata {
                content_kind: kind,
                content_kind_confidence: Some(1.0),
                ..Default::default()
            });
        }
    }

    let pipeline_request = build_pipeline_request_from_parts(
        &settings.resolution,
        settings.framegen,
        settings.hdr,
        &source_descriptor,
        quality_level,
    );
    let planner_capabilities = build_planner_capabilities(
        state,
        runtime_backend,
        &pipeline_request,
        settings.executor_preference,
    )
    .await;

    assemble_startup_context(
        runtime_backend.clone(),
        source_descriptor,
        pipeline_request,
        planner_capabilities,
        quality_level,
    )
}

pub(super) fn build_remote_proxy_session_info(
    local_session_id: &str,
    mut remote_session_info: SessionInfo,
) -> SessionInfo {
    remote_session_info.id = local_session_id.to_string();
    remote_session_info.output_url = local_output_url(local_session_id);
    remote_session_info.startup_complete = true;
    remote_session_info.retrying_startup = false;
    remote_session_info.startup_stage = "ready".to_string();
    remote_session_info
}

#[cfg(not(feature = "headless"))]
pub(super) fn maybe_show_stream_window(state: &ServerState, ui_settings: StreamUiSettings) {
    if !ui_settings.show_on_proxy_start {
        return;
    }

    if let Some(app) = state.app_handle.clone() {
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Ok(window) = crate::tray::get_or_create_window(&app2) {
                let _ = window.show();
                let _ = window.set_focus();
            }
        });
    }
}

#[cfg(feature = "headless")]
pub(super) fn maybe_show_stream_window(_state: &ServerState, _ui_settings: StreamUiSettings) {}

#[cfg(not(feature = "headless"))]
pub(super) fn maybe_show_stream_notification(
    state: &ServerState,
    ui_settings: StreamUiSettings,
    app_name: Option<&str>,
) {
    if !ui_settings.notifications {
        return;
    }

    if let Some(handle) = &state.app_handle {
        use tauri_plugin_notification::NotificationExt;
        let body = match app_name {
            Some(name) => format!("{} stream is live", name),
            None => "Stream is now live".to_string(),
        };
        let _ = handle
            .notification()
            .builder()
            .title("REFEREE")
            .body(body)
            .show();
    }
}

#[cfg(feature = "headless")]
pub(super) fn maybe_show_stream_notification(
    _state: &ServerState,
    _ui_settings: StreamUiSettings,
    _app_name: Option<&str>,
) {
}

/// Starts a new streaming session for the provided input URL and returns information about the created session.
///
/// Validates the request, resolves the runtime backend, stops any existing sessions, probes the source, plans and
/// starts the pipeline, and waits for the packager-owned playlist to become available. On success, the response
/// contains the session id, a URL to the session's playlist, the selected output resolution, optional detected
/// source resolution, and any provided app/stream title metadata.
///
/// On failure this returns an HTTP status code paired with a JSON error describing the failure (e.g., invalid
/// request, missing encoder/runtime, pipeline timeout, or premature pipeline exit).
///
/// # Returns
///
/// On success, a `StreamStartResponse` describing the newly created session.
/// On failure, a `(StatusCode, ErrorResponse)` tuple suitable for Axum.
///
/// # Examples
///
pub(super) async fn handle_stream_start(
    State(state): State<ServerState>,
    RemoteIp(ip_str): RemoteIp,
    Json(req): Json<StreamStartRequest>,
) -> HandlerResult<Json<StreamStartResponse>> {
    // Rate limit: 3 stream-start requests per minute per IP to prevent GPU DoS.
    let rate_key = format!("stream:{}", ip_str);
    if !state.rate_limit_stream.check(&rate_key) {
        return Err(error_response(
            StatusCode::TOO_MANY_REQUESTS,
            error_codes::RATE_LIMITED,
            "Too many stream requests. Please wait before retrying.",
        ));
    }

    validate_stream_start_request(&req)?;

    // SSRF protection: reject URLs that resolve to loopback or link-local addresses.
    // RFC-1918 private ranges are allowed so LAN stream sources remain reachable.
    if crate::source::is_ssrf_target(&req.url).await {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            error_codes::SSRF_BLOCKED,
            "The stream URL targets a blocked address.",
        ));
    }

    let (relay_settings, ui_settings) = {
        let settings = state.settings.read().await;
        (settings.relay.clone(), StreamUiSettings::from(&*settings))
    };
    let extra_headers = match req.headers.as_ref() {
        Some(headers) => {
            validate_forwarded_headers(headers)?;
            headers.clone()
        }
        None => HashMap::new(),
    };
    let app_name = normalize_optional_text(req.app_name.clone());
    let stream_title = normalize_optional_text(req.stream_title.clone());

    if let Some(relay_target) =
        resolve_active_relay_target(&relay_settings)
            .await
            .map_err(|error| {
                error_response(
                    StatusCode::BAD_GATEWAY,
                    error_codes::RELAY_UNAVAILABLE,
                    error,
                )
            })?
    {
        return handle_relay_stream_start(
            &state,
            ui_settings,
            &req,
            relay_target,
            app_name,
            stream_title,
        )
        .await
        .map(Json);
    }

    let local_settings = {
        let settings = state.settings.read().await;
        LocalStreamSettings::from(&*settings)
    };

    handle_local_stream_start(
        &state,
        local_settings,
        req,
        extra_headers,
        app_name,
        stream_title,
    )
    .await
    .map(Json)
}

pub(super) async fn handle_relay_stream_start(
    state: &ServerState,
    ui_settings: StreamUiSettings,
    req: &StreamStartRequest,
    relay_target: RelayControlTarget,
    app_name: Option<String>,
    stream_title: Option<String>,
) -> HandlerResult<StreamStartResponse> {
    let evicted_sessions = stop_existing_sessions(state).await.map_err(|error| {
        error_response(
            StatusCode::BAD_GATEWAY,
            error_codes::SESSION_STOP_FAILED,
            error,
        )
    })?;
    let session_id = uuid::Uuid::new_v4().to_string();

    maybe_show_stream_window(state, ui_settings);

    info!(
        "[Relay]: Forwarding stream start — session={} peer={} url={}",
        session_id,
        relay_target_display_name(&relay_target.selected_peer),
        req.url,
    );

    let remote_response = forward_remote_stream_start(req, &relay_target)
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_GATEWAY,
                error_codes::RELAY_START_FAILED,
                error,
            )
        })?;
    let remote_session_id = remote_response.session_id.clone();
    let remote_session_info =
        match fetch_remote_session_info(&relay_target, &remote_session_id).await {
            Ok(session_info) => session_info,
            Err(error) => {
                rollback_remote_session_start(&relay_target, &remote_session_id).await;
                return Err(error_response(
                    StatusCode::BAD_GATEWAY,
                    error_codes::RELAY_STATUS_SYNC_FAILED,
                    error,
                ));
            }
        };

    let local_session_info = build_remote_proxy_session_info(&session_id, remote_session_info);
    let response_resolution = local_session_info.output_resolution.clone();
    let response_source_resolution = local_session_info
        .source_resolution
        .clone()
        .or(remote_response.source_resolution);
    let response_app_name = local_session_info
        .app_name
        .clone()
        .or(app_name)
        .or(remote_response.app_name);
    let response_stream_title = local_session_info
        .stream_title
        .clone()
        .or(stream_title)
        .or(remote_response.stream_title);
    let response_quality = local_session_info.quality_level;

    let session = Session::new_remote(
        local_session_info,
        relay_target.base_url.clone(),
        remote_session_id,
        relay_target.remote_token.clone(),
        relay_target.selected_peer.clone(),
    );
    state.sessions.insert(session_id.clone(), session);

    emit_status_update(state).await;
    maybe_show_stream_notification(state, ui_settings, response_app_name.as_deref());

    Ok(StreamStartResponse {
        session_id: session_id.clone(),
        url: local_output_url(&session_id),
        resolution: response_resolution,
        source_resolution: response_source_resolution,
        app_name: response_app_name,
        stream_title: response_stream_title,
        effective_quality: response_quality,
        evicted_sessions,
    })
}

pub(super) async fn handle_local_stream_start(
    state: &ServerState,
    settings: LocalStreamSettings,
    req: StreamStartRequest,
    extra_headers: HashMap<String, String>,
    app_name: Option<String>,
    stream_title: Option<String>,
) -> HandlerResult<StreamStartResponse> {
    let gpu = state.gpu_info.read().await.clone();
    let runtime_backend = resolve_runtime_backend(&gpu)?;

    // Kill any active sessions before starting a new one — only one stream at a time.
    let evicted_sessions = stop_existing_sessions(state).await.map_err(|error| {
        error_response(
            StatusCode::BAD_GATEWAY,
            error_codes::SESSION_STOP_FAILED,
            error,
        )
    })?;

    let output_resolution = settings.resolution.clone();
    let quality_level = clamp_quality(&output_resolution, settings.quality);

    let session_id = uuid::Uuid::new_v4().to_string();
    let session_dir = state.tmp_dir.join(&session_id);
    std::fs::create_dir_all(&session_dir).ok();

    let startup_context = build_startup_context(
        state,
        &settings,
        &runtime_backend,
        &session_id,
        &req.url,
        &extra_headers,
        quality_level,
        req.content_kind,
    )
    .await;

    let packager_playlist_path = session_dir.join("index.m3u8");
    let profile = settings
        .encoding_profiles
        .get(&output_resolution)
        .cloned()
        .unwrap_or_else(|| get_encoding_profile(&output_resolution));
    let source_headers = startup_context.source_descriptor.session_headers.clone();

    if let Some(relayed_source) = &startup_context.source_descriptor.relay {
        info!(
            "[Input Proxy]: Session {} will proxy encoder input via {}",
            session_id, relayed_source.relay_url
        );
    }

    trace!(
        "[Pipeline]: Prepared session {} source={:?} request={:?} capabilities={:?} plan={:?}",
        session_id,
        startup_context.source_descriptor,
        startup_context.pipeline_request,
        startup_context.planner_capabilities,
        startup_context.execution_plan
    );

    maybe_show_stream_window(state, settings.ui);

    info!(
        "[Pipeline]: Stream starting \u{2014} session={} backend={} resolution={} url={}",
        session_id, startup_context.runtime_backend.encoder_backend, output_resolution, req.url,
    );
    log_source_content_decision(
        &session_id,
        &startup_context.source_descriptor,
        &startup_context.execution_plan,
    );

    let (hb_tx, hb_rx) = mpsc::channel::<()>(1);
    let completion = PipelineCompletionSignal::new();

    let session = Session::new_local(
        SessionInfo {
            id: session_id.clone(),
            source_url: req.url.clone(),
            output_url: local_output_url(&session_id),
            app_name: app_name.clone(),
            stream_title: stream_title.clone(),
            source_content_kind: detected_source_content_kind(&startup_context.source_descriptor),
            upscaler: active_upscaler_label(&startup_context.execution_plan),
            source_resolution: startup_context
                .source_descriptor
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.source_resolution.clone()),
            output_resolution: startup_context.pipeline_request.output_resolution.clone(),
            source_fps: startup_context.pipeline_request.source_fps,
            target_fps: startup_context.target_fps,
            framegen_enabled: startup_context.framegen_enabled,
            hdr_enabled: settings.hdr,
            quality_level: startup_context.quality_level,
            executor: startup_context.execution_plan.executor,
            encoder_backend: Some(startup_context.runtime_backend.encoder_backend.clone()),
            startup_complete: false,
            retrying_startup: false,
            startup_stage: "starting".to_string(),
        },
        source_headers,
        hb_tx,
        session_dir.clone(),
        completion.clone(),
        1,
        "low-latency",
    );

    state.sessions.insert(session_id.clone(), session);

    let streaming_rife_params = match (
        startup_context.runtime_backend.rife_worker_path.clone(),
        startup_context.runtime_backend.rife_model_path.clone(),
    ) {
        (Some(rife_worker_path), Some(model_path)) => Some(StreamingRifeParams {
            rife_worker_path,
            ffmpeg_path: startup_context.runtime_backend.ffmpeg_path.clone(),
            model_path,
        }),
        _ => None,
    };
    start_pipeline(
        &session_id,
        &startup_context.source_descriptor,
        &output_resolution,
        &profile,
        &startup_context.execution_plan,
        &packager_playlist_path,
        &startup_context.runtime_backend.gpu_vendor,
        &startup_context.runtime_backend.encoder_backend,
        &startup_context.runtime_backend.encoder_path,
        &startup_context.runtime_backend.ffmpeg_path,
        streaming_rife_params,
        &state.sessions,
        &session_dir,
        completion,
        hb_rx,
    );

    // Session startup completion is gated on the packager-owned final playlist,
    // even when the current backend happens to emit that playlist inline.
    match wait_for_packager_playlist(
        &packager_playlist_path,
        &session_id,
        &state.sessions,
        180_000,
    )
    .await
    {
        PlaylistWaitOutcome::Ready => {}
        PlaylistWaitOutcome::SessionEnded => {
            return Err(error_response(
                StatusCode::BAD_GATEWAY,
                error_codes::PIPELINE_EXITED,
                "Pipeline exited before output became ready.",
            ));
        }
        PlaylistWaitOutcome::TimedOut => {
            cleanup_session(&session_id, &state.sessions).await;
            return Err(error_response(
                StatusCode::GATEWAY_TIMEOUT,
                error_codes::PIPELINE_TIMEOUT,
                "Pipeline timed out waiting for the GPU.",
            ));
        }
    }

    if let Some(mut session) = state.sessions.get_mut(&session_id) {
        // Preserve the existing client-facing flag, but treat it as the moment
        // the packager-owned final playlist becomes readable.
        session.mark_startup_ready();
    }

    emit_status_update(state).await;

    maybe_show_stream_notification(state, settings.ui, app_name.as_deref());

    let source_resolution = state
        .sessions
        .get(&session_id)
        .and_then(|s| s.info.source_resolution.clone());

    Ok(StreamStartResponse {
        session_id: session_id.clone(),
        url: local_output_url(&session_id),
        resolution: output_resolution,
        source_resolution,
        app_name,
        stream_title,
        effective_quality: quality_level,
        evicted_sessions,
    })
}

pub(super) async fn handle_heartbeat(
    State(state): State<ServerState>,
    AxumPath(session_id): AxumPath<String>,
) -> HandlerResult<Json<OkResponse>> {
    if let Some(session) = state.sessions.get(&session_id) {
        // A full channel (previous heartbeat not yet consumed) is fine — the session is alive.
        // Remote-backed sessions do not own a local heartbeat channel, so this
        // becomes a no-op until forwarded relay heartbeats are added.
        session.try_send_heartbeat();
        let remote = session.remote_backing().cloned();
        drop(session);

        if let Some(remote) = remote {
            match forward_remote_heartbeat_request(&remote).await {
                Ok(true) => {}
                Ok(false) => {
                    cleanup_session(&session_id, &state.sessions).await;
                    emit_status_update(&state).await;
                    return Err(error_response(
                        StatusCode::NOT_FOUND,
                        error_codes::SESSION_NOT_FOUND,
                        "Relay-backed session is no longer active on the linked REFEREE instance.",
                    ));
                }
                Err(error) => {
                    return Err(error_response(
                        StatusCode::BAD_GATEWAY,
                        error_codes::RELAY_HEARTBEAT_FAILED,
                        error,
                    ));
                }
            }
        }
        Ok(Json(OkResponse {
            status: "ok".to_string(),
        }))
    } else {
        Err(error_response(
            StatusCode::NOT_FOUND,
            error_codes::SESSION_NOT_FOUND,
            "Session not found",
        ))
    }
}

/// Resolves the target session ID from an optional requested ID.
///
/// - If `requested_id` is `Some(id)`, verifies the session exists and returns `Ok(id)`.
/// - If `requested_id` is `None`, returns the first session found in the map (useful for
///   single-session use cases where the caller does not know the ID).
/// - Returns a 404 error response if no matching session can be found.
pub(super) fn resolve_session_id(
    sessions: &SessionMap,
    requested_id: String,
) -> HandlerResult<String> {
    if sessions.contains_key(&requested_id) {
        Ok(requested_id)
    } else {
        Err(error_response(
            StatusCode::NOT_FOUND,
            error_codes::SESSION_NOT_FOUND,
            "Session not found",
        ))
    }
}

pub(super) async fn handle_stream_stop(
    State(state): State<ServerState>,
    Json(req): Json<StopRequest>,
) -> HandlerResult<Json<OkResponse>> {
    if req.stop_all {
        let ids: Vec<String> = state.sessions.iter().map(|e| e.key().clone()).collect();
        for id in &ids {
            stop_tracked_session(id, &state.sessions)
                .await
                .map_err(|error| {
                    error_response(
                        StatusCode::BAD_GATEWAY,
                        error_codes::RELAY_STOP_FAILED,
                        error,
                    )
                })?;
        }
    } else {
        let session_id = req.session_id.ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_REQUEST,
                "Either sessionId or stopAll must be provided.",
            )
        })?;

        let session_id = resolve_session_id(&state.sessions, session_id)?;
        let stopped_app_name = stop_tracked_session(&session_id, &state.sessions)
            .await
            .map_err(|error| {
                error_response(
                    StatusCode::BAD_GATEWAY,
                    error_codes::RELAY_STOP_FAILED,
                    error,
                )
            })?;
        #[cfg(feature = "headless")]
        let _ = &stopped_app_name;

        #[cfg(not(feature = "headless"))]
        {
            let settings = state.settings.read().await.clone();
            if settings.notifications {
                if let Some(handle) = &state.app_handle {
                    use tauri_plugin_notification::NotificationExt;
                    let body = match stopped_app_name.as_deref() {
                        Some(name) => format!("{} stream stopped", name),
                        None => "Stream stopped".to_string(),
                    };
                    let _ = handle
                        .notification()
                        .builder()
                        .title("REFEREE")
                        .body(body)
                        .show();
                }
            }
        }
    }

    emit_status_update(&state).await;

    Ok(Json(OkResponse {
        status: "stopped".to_string(),
    }))
}

pub(super) async fn serve_tmp_file(
    State(state): State<ServerState>,
    AxumPath((session_id, filename)): AxumPath<(String, String)>,
    Query(req): Query<TmpFileRequest>,
) -> Result<axum::response::Response, StatusCode> {
    let session_dir = match state.sessions.get(&session_id) {
        Some(session) => {
            if let Some(remote) = session.remote_backing().cloned() {
                drop(session);
                return serve_remote_tmp_file(&session_id, &filename, &remote, &req).await;
            }

            match session.session_dir() {
                Some(dir) => dir.to_path_buf(),
                None => return Err(StatusCode::BAD_GATEWAY),
            }
        }
        None => return Err(StatusCode::NOT_FOUND),
    };
    let file_path = session_dir.join(&filename);

    if !file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Guard against path traversal: ensure the resolved path is still within
    // the session's directory before serving any bytes.
    let canonical_file = file_path
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let canonical_dir = session_dir
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !canonical_file.starts_with(&canonical_dir) {
        return Err(StatusCode::NOT_FOUND);
    }

    let headers = tmp_file_response_headers(&filename);
    match tokio::fs::read(&canonical_file).await {
        Ok(data) => {
            let mut builder = axum::response::Response::builder()
                .header("Content-Type", headers.content_type)
                .header("Cache-Control", headers.cache_control)
                .header("Access-Control-Allow-Origin", "*");

            if headers.legacy_no_cache {
                builder = builder.header("Pragma", "no-cache").header("Expires", "0");
            }

            Ok(builder
                .body(axum::body::Body::from(data))
                .expect("response body construction is infallible for Vec<u8>"))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub(super) async fn handle_input_proxy(
    State(state): State<ServerState>,
    AxumPath(session_id): AxumPath<String>,
    Query(req): Query<InputProxyRequest>,
) -> Result<axum::response::Response, StatusCode> {
    let source_headers = match state.sessions.get(&session_id) {
        Some(session) => match session.source_headers() {
            Some(headers) => headers.clone(),
            None => return Err(StatusCode::BAD_GATEWAY),
        },
        None => return Err(StatusCode::NOT_FOUND),
    };

    hls_relay::relay_request(
        INPUT_PROXY_HOST,
        PORT,
        &session_id,
        &source_headers,
        &req.url,
    )
    .await
}

pub(super) fn build_remote_tmp_proxy_url(session_id: &str, target_url: &reqwest::Url) -> String {
    let filename = target_url
        .path_segments()
        .and_then(|segments| segments.last())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("proxy");
    let mut proxy_url = reqwest::Url::parse(&format!(
        "http://localhost:{}/v1/tmp/{}/{}",
        PORT, session_id, filename
    ))
    .expect("remote tmp proxy base URL should be valid");
    proxy_url
        .query_pairs_mut()
        .append_pair("url", target_url.as_str());
    proxy_url.to_string()
}

pub(super) fn build_remote_tmp_target_url(
    remote: &RemoteSessionBacking,
    filename: &str,
    requested_url: Option<&str>,
) -> Result<reqwest::Url, StatusCode> {
    let base_url =
        reqwest::Url::parse(&remote.remote_base_url).map_err(|_| StatusCode::BAD_GATEWAY)?;
    let allowed_prefix = format!("/v1/tmp/{}/", remote.remote_session_id);

    let target_url = match requested_url {
        Some(raw) => reqwest::Url::parse(raw).map_err(|_| StatusCode::BAD_REQUEST)?,
        None => reqwest::Url::parse(&format!(
            "{}/v1/tmp/{}/{}",
            remote.remote_base_url.trim_end_matches('/'),
            remote.remote_session_id,
            filename
        ))
        .map_err(|_| StatusCode::BAD_GATEWAY)?,
    };

    if !matches!(target_url.scheme(), "http" | "https") {
        return Err(StatusCode::BAD_REQUEST);
    }

    if target_url.scheme() != base_url.scheme()
        || target_url.host_str() != base_url.host_str()
        || target_url.port_or_known_default() != base_url.port_or_known_default()
        || !target_url.path().starts_with(&allowed_prefix)
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(target_url)
}

pub(super) async fn serve_remote_tmp_file(
    session_id: &str,
    filename: &str,
    remote: &RemoteSessionBacking,
    request: &TmpFileRequest,
) -> Result<axum::response::Response, StatusCode> {
    let target_url = build_remote_tmp_target_url(remote, filename, request.url.as_deref())?;
    let upstream = remote_tmp_proxy_http_client()
        .map_err(|error| {
            tracing::warn!("[Relay Proxy]: {}", error);
            StatusCode::BAD_GATEWAY
        })?
        .get(target_url.clone())
        .header("X-Referee-Token", remote.remote_token.as_str())
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(
                "[Relay Proxy]: Failed to fetch remote tmp resource for session {}: {}",
                session_id,
                error
            );
            StatusCode::BAD_GATEWAY
        })?;

    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let response_headers = tmp_file_response_headers(filename);

    if hls_relay::is_hls_manifest_url(&target_url)
        || hls_relay::is_hls_manifest_content_type(content_type.as_deref())
    {
        let body = upstream.text().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
        let rewritten =
            hls_relay::rewrite_hls_manifest_with_target_resolver(&target_url, &body, |resolved| {
                Some(build_remote_tmp_proxy_url(session_id, resolved))
            });
        let manifest_content_type = content_type
            .as_deref()
            .unwrap_or(response_headers.content_type);

        let mut builder = axum::response::Response::builder()
            .status(status)
            .header(axum::http::header::CONTENT_TYPE, manifest_content_type)
            .header("Cache-Control", response_headers.cache_control)
            .header("Access-Control-Allow-Origin", "*");

        if response_headers.legacy_no_cache {
            builder = builder.header("Pragma", "no-cache").header("Expires", "0");
        }

        return Ok(builder
            .body(axum::body::Body::from(rewritten))
            .expect("response body construction is infallible for String"));
    }

    let mut builder = axum::response::Response::builder()
        .status(status)
        .header("Cache-Control", response_headers.cache_control)
        .header("Access-Control-Allow-Origin", "*");

    if let Some(content_type) = content_type.as_deref() {
        builder = builder.header(axum::http::header::CONTENT_TYPE, content_type);
    } else {
        builder = builder.header(
            axum::http::header::CONTENT_TYPE,
            response_headers.content_type,
        );
    }

    if response_headers.legacy_no_cache {
        builder = builder.header("Pragma", "no-cache").header("Expires", "0");
    }

    Ok(builder
        .body(axum::body::Body::from_stream(upstream.bytes_stream()))
        .expect("response body construction is infallible for upstream stream"))
}
