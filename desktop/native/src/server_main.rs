mod deps;
mod exec;
mod gpu;
mod graph;
mod normalize;
mod pipeline;
mod preprocess;
mod process;
mod runtime;
mod server;
mod settings;
mod source;

use dashmap::DashMap;
use server::{load_approved_origins, RateLimiter, ServerSettings, ServerState};
use settings::{ApprovedOriginMeta, SettingsManager};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Resolves the same base data directory Tauri would use for com.referee.proxy.
/// Windows: %APPDATA%\com.referee.proxy
/// Linux:   $HOME/.local/share/com.referee.proxy
fn app_data_dir() -> (std::path::PathBuf, Vec<&'static str>) {
    let mut startup_warnings = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| {
            // Fallback for Docker containers or environments where APPDATA is unset.
            startup_warnings.push(
                "[Main]: APPDATA env var not set; falling back to temp dir for data storage.",
            );
            std::env::temp_dir().to_string_lossy().into_owned()
        });
        (
            std::path::PathBuf::from(appdata).join("com.referee.proxy"),
            startup_warnings,
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| {
            startup_warnings
                .push("[Main]: HOME env var not set; falling back to /tmp for data storage.");
            "/tmp".to_string()
        });
        (
            std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("com.referee.proxy"),
            startup_warnings,
        )
    }
}

/// Initializes encoder binaries and detects encoder capabilities for a
/// headless server based on the current GPU information.
///
/// This function ensures encoder binaries are present for the detected GPU,
/// refreshes GPU detection after installation, and writes detected capability
/// flags into `server_state.encoder_capabilities`. If no supported GPU is
/// detected or a binary download fails, it returns without writing capabilities.
///
/// # Parameters
///
/// - `server_state` — shared server state which contains `gpu_info` and will be updated with `encoder_capabilities`.
/// - `lib_dir` — filesystem directory where encoder binaries and related libraries are stored or downloaded to.
/// - `http_client` — HTTP client used to download encoder binaries when they are missing.
///
/// # Examples
///
async fn bootstrap_headless_setup(
    server_state: &ServerState,
    lib_dir: &std::path::Path,
    http_client: &reqwest::Client,
) {
    let initial_gpu = server_state.gpu_info.read().await.clone();

    if initial_gpu.vendor == "unknown" {
        warn!("[Setup]: No supported GPU detected during startup; skipping encoder setup.");
        return;
    }

    if !deps::binaries_ready(lib_dir, &initial_gpu.vendor) {
        info!(
            "[Setup]: Downloading encoder binaries for {:?} {:?}",
            initial_gpu.vendor, initial_gpu.name
        );

        let result =
            deps::download_binaries(http_client, lib_dir, &initial_gpu.vendor, |progress| {
                info!("[Setup]: {} ({}%)", progress.detail, progress.percent);
            })
            .await;

        if let Err(download_error) = result {
            error!("[Setup]: Binary download failed: {}", download_error);
            return;
        }

        let lib_dir_spawn = lib_dir.to_path_buf();
        let refreshed_gpu =
            tokio::task::spawn_blocking(move || gpu::detect_compatible_gpu(&lib_dir_spawn))
                .await
                .unwrap_or_default();
        info!("[Setup]: GPU refreshed after setup: {:?}", refreshed_gpu);
        *server_state.gpu_info.write().await = refreshed_gpu;
    }

    let (backend, encoder_path, rife_worker_path) = {
        let gpu = server_state.gpu_info.read().await;
        (
            gpu.backend.clone(),
            gpu.encoder_path.clone(),
            gpu.rife_worker_path.clone(),
        )
    };

    if let (Some(backend), Some(encoder_path)) = (backend, encoder_path) {
        let rife = rife_worker_path
            .as_deref()
            .map(pipeline::detect_rife_capability)
            .unwrap_or(false);
        let capabilities =
            pipeline::detect_encoder_capabilities(&encoder_path, &backend).with_rife(rife);
        info!(
            "[Setup]: Encoder capabilities — fruc={}, truehdr={}, rife={}",
            capabilities.has_fruc, capabilities.has_truehdr, capabilities.has_rife
        );
        *server_state.encoder_capabilities.write().await = Some(capabilities);
    } else {
        warn!("[Setup]: GPU detected, but no encoder binary is ready yet.");
    }
}

fn persisted_or_new_uuid(slot: &mut Option<String>, changed: &mut bool) -> String {
    match slot.clone() {
        Some(value) => value,
        None => {
            let value = uuid::Uuid::new_v4().to_string();
            *slot = Some(value.clone());
            *changed = true;
            value
        }
    }
}

fn init_auth_token(settings_manager: &SettingsManager) -> (String, String) {
    let mut settings = settings_manager.get();

    if let Ok(env_token) = std::env::var("REFEREE_API_TOKEN") {
        let env_token = env_token.trim().to_string();
        if env_token.len() < 32 {
            error!("[Main]: REFEREE_API_TOKEN must be at least 32 characters. Ignoring.");
            let mut changed = false;
            let token = persisted_or_new_uuid(&mut settings.api_token, &mut changed);
            let instance_id = persisted_or_new_uuid(&mut settings.instance_id, &mut changed);
            if changed {
                settings_manager.update(settings);
            }
            return (token, instance_id);
        }

        // Persist so headed restarts also pick up the env-specified token.
        settings.api_token = Some(env_token.clone());
        let mut changed = true;
        let instance_id = persisted_or_new_uuid(&mut settings.instance_id, &mut changed);
        settings_manager.update(settings);
        info!("[Main]: API token loaded from REFEREE_API_TOKEN environment variable.");
        return (env_token, instance_id);
    }

    let mut changed = false;
    let token = persisted_or_new_uuid(&mut settings.api_token, &mut changed);
    let instance_id = persisted_or_new_uuid(&mut settings.instance_id, &mut changed);
    if changed {
        settings_manager.update(settings);
    }
    (token, instance_id)
}

/// Application entry point for the headless Referee Proxy server.
///
/// Performs one-time startup and long-running orchestration: ensures runtime directories and shader assets; initializes
/// logging and settings; builds an HTTP client; detects GPU and constructs shared server state;
/// runs bootstrapping for encoder binaries and capabilities; spawns a background GPU refresh loop;
/// starts the HTTP server; and handles graceful shutdown by cleaning up active sessions on SIGINT.
///
/// # Examples
///
#[tokio::main]
async fn main() {
    let (app_data, startup_warnings) = app_data_dir();
    let lib_dir = app_data.join("lib");
    let config_path = app_data.join("config.json");
    let log_dir = app_data.join("logs");
    let tmp_dir = std::env::temp_dir().join("RefereeProxy");

    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&app_data).ok();
    std::fs::create_dir_all(&lib_dir).ok();
    if let Err(e) = deps::ensure_universal_shaders(&lib_dir) {
        warn!("[Main]: Failed to install Universal shader assets: {}", e);
    }
    std::fs::create_dir_all(&log_dir).ok();
    std::fs::create_dir_all(&tmp_dir).ok();

    // Rolling daily log file, same scheme as the desktop app.
    let file_appender = tracing_appender::rolling::daily(&log_dir, "referee-server.log");
    let (non_blocking_writer, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("referee=info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking_writer))
        .init();

    for warning in startup_warnings {
        warn!("{}", warning);
    }

    if let Err(error) = server::validate_http_clients() {
        error!("[Main]: {}", error);
        std::process::exit(1);
    }

    let settings_manager = Arc::new(SettingsManager::new(&config_path));

    let (api_token, instance_id) = init_auth_token(&settings_manager);

    let initial_settings = settings_manager.get();
    let http_client = match reqwest::Client::builder()
        .user_agent("REFEREE-Upscaler")
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            error!("[Main]: Failed to build setup HTTP client: {}", error);
            std::process::exit(1);
        }
    };

    let lib_dir_spawn = lib_dir.clone();
    let gpu_info = tokio::task::spawn_blocking(move || gpu::detect_compatible_gpu(&lib_dir_spawn))
        .await
        .unwrap_or_default();
    info!("[Main]: GPU detected: {:?}", gpu_info);

    let approved_origins_path = app_data.join("approved-origins.json");
    let server_state = ServerState {
        sessions: pipeline::new_session_map(),
        instance_id,
        gpu_info: Arc::new(RwLock::new(gpu_info.clone())),
        settings: Arc::new(RwLock::new(ServerSettings::from(&initial_settings))),
        encoder_capabilities: Arc::new(RwLock::new(None)),
        tmp_dir,
        app_handle: None,
        api_token: Arc::new(std::sync::RwLock::new(api_token)),
        pending_consents: Arc::new(DashMap::new()),
        approved_origins: Arc::new(load_approved_origins(&approved_origins_path)),
        approved_origins_path: Some(approved_origins_path),
        settings_path: Some(config_path.clone()),
        settings_manager: Some(settings_manager.clone()),
        rate_limit_auth: Arc::new(RateLimiter::new(5, 60)),
        rate_limit_stream: Arc::new(RateLimiter::new(3, 60)),
        pending_consent_ui: Arc::new(std::sync::Mutex::new(None)),
        setup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    // Log a masked hint (last 4 chars only) — the full token must be supplied via
    // REFEREE_API_TOKEN or read from the persisted config.json.
    let token_hint = {
        let t = server_state.api_token.read().unwrap();
        if t.len() >= 4 {
            format!("...{}", &t[t.len() - 4..])
        } else {
            "****".to_string()
        }
    };
    info!(
        "[Main]: API token initialised (hint: {}). To use a custom token set REFEREE_API_TOKEN.",
        token_hint
    );

    // Pre-approve origins supplied via REFEREE_ALLOWED_ORIGINS (comma-separated list of
    // http/https origins). This unblocks the /v1/auth/request immediate-grant path for
    // callers that the operator has already authorised before container start.
    if let Ok(env_origins) = std::env::var("REFEREE_ALLOWED_ORIGINS") {
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let mut count = 0usize;
        for raw in env_origins.split(',') {
            let origin = raw.trim().to_string();
            if origin.is_empty() {
                continue;
            }
            if !origin.starts_with("http://") && !origin.starts_with("https://") {
                warn!(
                    concat!(
                        "[Main]: REFEREE_ALLOWED_ORIGINS: skipping invalid origin {:?} ",
                        "(must start with http:// or https://)"
                    ),
                    origin
                );
                continue;
            }
            server_state
                .approved_origins
                .entry(origin)
                .or_insert_with(|| {
                    count += 1;
                    ApprovedOriginMeta {
                        app_name: None,
                        approved_at: now_ts.clone(),
                    }
                });
        }
        if count > 0 {
            if let Some(ref path) = server_state.approved_origins_path {
                server::persist_approved_origins(&server_state.approved_origins, path);
            }
            info!(
                "[Main]: Auto-approved {} origin(s) from REFEREE_ALLOWED_ORIGINS.",
                count
            );
        }
    }

    bootstrap_headless_setup(&server_state, &lib_dir, &http_client).await;
    server_state
        .setup_complete
        .store(true, std::sync::atomic::Ordering::Release);

    // GPU refresh loop — re-detect every 60 s so encoder_path stays accurate.
    let gpu_arc = server_state.gpu_info.clone();
    let lib_dir_for_refresh = lib_dir.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let lib_dir_spawn = lib_dir_for_refresh.clone();
            let new_info =
                tokio::task::spawn_blocking(move || gpu::detect_compatible_gpu(&lib_dir_spawn))
                    .await
                    .unwrap_or_default();
            *gpu_arc.write().await = new_info;
        }
    });

    let shutdown_sessions = server_state.sessions.clone();
    tokio::select! {
        _ = server::start_server(server_state) => {}
        result = tokio::signal::ctrl_c() => {
            match result {
                Ok(()) => info!("[Main]: Shutdown signal received; cleaning up active sessions."),
                Err(error) => warn!("[Main]: Failed to listen for shutdown signal: {}", error),
            }
            server::shutdown(&shutdown_sessions).await;
        }
    }
}
