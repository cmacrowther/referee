use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub(super) gpu_ready: Option<bool>,
    pub(super) gpu_name: Option<String>,
    pub(super) gpu_vendor: Option<String>,
    pub(super) gpu_utilization: Option<u8>,
    pub(super) encoder_backend: Option<String>,
    pub(super) selected_executor: Option<ExecutorKind>,
    pub(super) nvidia_ai_available: Option<bool>,
    pub(super) amd_ai_available: Option<bool>,
    pub(super) active_sessions: usize,
    pub(super) sessions: Vec<SessionInfo>,
    pub(super) primary_session: Option<SessionInfo>,
    pub(super) settings: SettingsSnapshot,
    pub(super) encoder_has_framegen: Option<bool>,
    pub(super) encoder_has_truehdr: Option<bool>,
    pub(super) encoder_has_rife: Option<bool>,
}

#[derive(Serialize)]
pub struct SettingsSnapshot {
    resolution: String,
    quality: u8,
    framegen: bool,
    hdr: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PingResponse {
    instance_id: String,
    hostname: String,
    version: String,
    platform: String,
    gpu_ready: bool,
    gpu_vendor: String,
    gpu_name: Option<String>,
}

pub(super) async fn handle_ping(State(state): State<ServerState>) -> Json<PingResponse> {
    let gpu = state.gpu_info.read().await.clone();
    let gpu_ready = local_gpu_ready_for_status(&gpu, state.setup_complete.load(Ordering::Acquire));

    Json(PingResponse {
        instance_id: state.instance_id.clone(),
        hostname: sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string()),
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        gpu_ready,
        gpu_vendor: gpu.vendor,
        gpu_name: gpu.name,
    })
}

pub(super) fn build_settings_executor_request(settings: &ServerSettings) -> PipelineRequest {
    PipelineRequest {
        source_transport: SourceTransport::RemoteHttp,
        source_kind: SourceKind::Hls,
        source_content_kind: SourceContentKind::Unknown,
        source_resolution: None,
        output_resolution: settings.resolution.clone(),
        source_fps: None,
        latency_mode: LatencyMode::Low,
        upscale: UpscaleRequest::Quality(clamp_quality(&settings.resolution, settings.quality)),
        interpolation: if settings.framegen {
            InterpolationRequest::To60
        } else {
            InterpolationRequest::Off
        },
        hdr: if settings.hdr {
            HdrRequest::TonemapToHdr10
        } else {
            HdrRequest::Off
        },
    }
}

pub(super) fn selected_executor_for_status(
    gpu: &GpuInfo,
    settings: &ServerSettings,
    capabilities: Option<&EncoderCapabilities>,
) -> Option<ExecutorKind> {
    if !has_supported_local_gpu(gpu) {
        return None;
    }

    let capabilities = capabilities?;
    let request = build_settings_executor_request(settings);

    Some(pipeline::select_executor_kind_for_request(
        ExecutorSelectionContext {
            platform: crate::exec::RuntimePlatform::current(),
            gpu_vendor: &gpu.vendor,
            request: &request,
            encoder_capabilities: capabilities,
            executor_preference: settings.executor_preference,
        },
    ))
}

#[cfg(not(feature = "headless"))]
pub(super) async fn emit_status_update(state: &ServerState) {
    let gpu = state.gpu_info.read().await.clone();
    let settings = state.settings.read().await.clone();
    let caps = state.encoder_capabilities.read().await.clone();
    let setup_complete = state.setup_complete.load(Ordering::Acquire);
    let status = build_status(
        &gpu,
        &settings,
        &state.sessions,
        caps.as_ref(),
        setup_complete,
    );
    let status = apply_status_readiness(status, &gpu, &settings, setup_complete).await;
    let status = enrich_status_with_remote_processing_stats(status, &state.sessions).await;
    if let Some(handle) = &state.app_handle {
        let _ = handle.emit("status-update", &status);
    }
}

#[cfg(feature = "headless")]
pub(super) async fn emit_status_update(_state: &ServerState) {}

/// Builds a status snapshot from the current GPU, server settings, active sessions, and optional encoder capabilities.
///
/// The returned status includes GPU readiness and metadata, the chosen executor,
/// per-vendor AI availability flags, active sessions, a settings snapshot, and
/// encoder capability flags.
///
/// # Returns
///
/// A `StatusResponse` populated from the provided inputs.
///
/// # Examples
///
pub(crate) fn build_status(
    gpu: &GpuInfo,
    settings: &ServerSettings,
    sessions_map: &SessionMap,
    capabilities: Option<&EncoderCapabilities>,
    setup_complete: bool,
) -> StatusResponse {
    let session_infos: Vec<SessionInfo> = sessions_map
        .iter()
        .map(|e| e.value().info.clone())
        .collect();
    let primary = session_infos.first().cloned();
    let selected_executor = primary
        .as_ref()
        .map(|session| session.executor)
        .or_else(|| selected_executor_for_status(gpu, settings, capabilities));
    let nvidia_ai_available = capabilities.map(|capabilities| {
        has_supported_local_gpu(gpu)
            && pipeline::nvidia_ai_available(
                crate::exec::RuntimePlatform::current(),
                &gpu.vendor,
                capabilities,
            )
    });
    let amd_ai_available = capabilities.map(|capabilities| {
        has_supported_local_gpu(gpu)
            && pipeline::amd_ai_available(
                crate::exec::RuntimePlatform::current(),
                &gpu.vendor,
                capabilities,
            )
    });

    StatusResponse {
        gpu_ready: Some(local_gpu_ready_for_status(gpu, setup_complete)),
        gpu_name: gpu.name.clone(),
        gpu_vendor: Some(gpu.vendor.clone()),
        gpu_utilization: gpu.utilization,
        encoder_backend: primary
            .as_ref()
            .and_then(|session| session.encoder_backend.clone())
            .or_else(|| {
                has_supported_local_gpu(gpu)
                    .then(|| gpu.backend.clone())
                    .flatten()
            }),
        selected_executor,
        nvidia_ai_available,
        amd_ai_available,
        active_sessions: sessions_map.len(),
        sessions: session_infos,
        primary_session: primary.clone(),
        settings: SettingsSnapshot {
            resolution: settings.resolution.clone(),
            quality: settings.quality,
            framegen: settings.framegen,
            hdr: settings.hdr,
        },
        encoder_has_framegen: capabilities.map(|c| c.has_fruc),
        encoder_has_truehdr: capabilities.map(|c| c.has_truehdr),
        encoder_has_rife: capabilities.map(|c| c.has_rife),
    }
}

pub(super) fn remote_backing_for_status(
    status: &StatusResponse,
    sessions_map: &SessionMap,
) -> Option<(String, RemoteSessionBacking)> {
    let local_session_id = status
        .primary_session
        .as_ref()
        .map(|session| session.id.clone())
        .or_else(|| status.sessions.first().map(|session| session.id.clone()))?;
    let session = sessions_map.get(&local_session_id)?;
    let remote = session.remote_backing()?.clone();
    Some((local_session_id, remote))
}

pub(super) fn apply_remote_status_to_local_status(
    status: &mut StatusResponse,
    local_session_id: &str,
    remote: &RemoteSessionBacking,
    mut remote_status: RemoteStatusResponse,
) {
    let remote_session_info =
        take_remote_session_info(&mut remote_status, &remote.remote_session_id);

    status.gpu_ready = remote_status.gpu_ready;
    status.gpu_name = remote_status.gpu_name;
    status.gpu_vendor = remote_status.gpu_vendor;
    status.gpu_utilization = remote_status.gpu_utilization;
    status.encoder_backend = remote_status
        .encoder_backend
        .or_else(|| remote_session_info.as_ref()?.encoder_backend.clone());
    status.selected_executor = remote_status
        .selected_executor
        .or_else(|| remote_session_info.as_ref().map(|session| session.executor));
    status.nvidia_ai_available = remote_status.nvidia_ai_available;
    status.amd_ai_available = remote_status.amd_ai_available;
    status.encoder_has_framegen = remote_status.encoder_has_framegen;
    status.encoder_has_truehdr = remote_status.encoder_has_truehdr;
    status.encoder_has_rife = remote_status.encoder_has_rife;

    if let Some(remote_session_info) = remote_session_info {
        let local_session_info =
            build_remote_proxy_session_info(local_session_id, remote_session_info);
        if let Some(index) = status
            .sessions
            .iter()
            .position(|session| session.id == local_session_id)
        {
            status.sessions[index] = local_session_info.clone();
        } else {
            status.sessions = vec![local_session_info.clone()];
        }
        status.primary_session = Some(local_session_info);
    }
}

pub(crate) async fn enrich_status_with_remote_processing_stats(
    mut status: StatusResponse,
    sessions_map: &SessionMap,
) -> StatusResponse {
    let Some((local_session_id, remote)) = remote_backing_for_status(&status, sessions_map) else {
        return status;
    };

    match fetch_remote_status(&remote.remote_base_url).await {
        Ok(remote_status) => {
            apply_remote_status_to_local_status(
                &mut status,
                &local_session_id,
                &remote,
                remote_status,
            );
        }
        Err(error) => {
            tracing::warn!(
                "[Relay]: Failed to refresh processing stats from linked REFEREE session {}: {}",
                remote.remote_session_id,
                error
            );
        }
    }

    status
}

pub(super) async fn handle_status(State(state): State<ServerState>) -> Json<StatusResponse> {
    let gpu = state.gpu_info.read().await.clone();
    let settings = state.settings.read().await.clone();
    let caps = state.encoder_capabilities.read().await.clone();
    let setup_complete = state.setup_complete.load(Ordering::Acquire);
    let status = build_status(
        &gpu,
        &settings,
        &state.sessions,
        caps.as_ref(),
        setup_complete,
    );
    let status = apply_status_readiness(status, &gpu, &settings, setup_complete).await;
    Json(enrich_status_with_remote_processing_stats(status, &state.sessions).await)
}
