pub mod graph;
pub mod source;

#[cfg(not(feature = "headless"))]
mod commands;
#[cfg(not(feature = "headless"))]
mod deps;
#[cfg(not(feature = "headless"))]
mod exec;
#[cfg(not(feature = "headless"))]
mod gpu;
#[cfg(not(feature = "headless"))]
mod normalize;
#[cfg(not(feature = "headless"))]
mod paths;
#[cfg(not(feature = "headless"))]
mod pipeline;
#[cfg(not(feature = "headless"))]
mod preprocess;
mod process;
#[cfg(not(feature = "headless"))]
mod runtime;
#[cfg(not(feature = "headless"))]
mod server;
#[cfg(not(feature = "headless"))]
mod settings;
#[cfg(not(feature = "headless"))]
mod tray;

#[cfg(not(feature = "headless"))]
use commands::{AppState, SetupLifecycle};
#[cfg(not(feature = "headless"))]
use dashmap::DashMap;
#[cfg(not(feature = "headless"))]
use server::{load_approved_origins, ServerSettings, ServerState};
#[cfg(not(feature = "headless"))]
use settings::SettingsManager;
#[cfg(not(feature = "headless"))]
use std::sync::{Arc, OnceLock};
#[cfg(not(feature = "headless"))]
use tauri::{Emitter, Manager};
#[cfg(not(feature = "headless"))]
use tokio::sync::{Notify, RwLock};
#[cfg(not(feature = "headless"))]
use tracing::{error, info, warn};
#[cfg(not(feature = "headless"))]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(not(feature = "headless"))]
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

#[cfg(feature = "headless")]
pub fn run() {
    panic!("desktop UI is unavailable when built with the `headless` feature");
}

/// Starts and runs the desktop Tauri application, wiring up native integrations,
/// application state, background services, and the system tray.
///
/// This entrypoint performs platform-specific environment adjustments (Linux),
/// initializes logging, paths, settings, GPU detection, and shared state,
/// registers invoke command handlers, spawns background tasks (HTTP server,
/// GPU refresh, renderer status pushes), and enters the Tauri run loop.
///
/// # Examples
///
#[cfg(not(feature = "headless"))]
pub fn run() {
    // On Linux (AppImage), the WebKitGTK sandbox subprocess fails under the
    // AppImage seccomp restrictions, causing a persistent blank webview.
    // Disable compositing mode to force a software path that works reliably.
    #[cfg(target_os = "linux")]
    {
        // SAFETY: set_var is technically UB when called from a multi-threaded
        // process (Rust 1.81+), but this runs inside tauri::Builder::default()
        // before Tauri spawns any additional threads, so the environment is
        // single-threaded at this point.
        std::env::set_var("GDK_BACKEND", "x11");
        //std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        // Disable the WebKitGTK sandbox subprocess — it fails under AppImage's
        // seccomp restrictions and silently produces a blank webview.
        //std::env::set_var("WEBKIT_FORCE_SANDBOX", "0");
        // Disable the DMA-BUF renderer — on Wayland (e.g. Bazzite/KDE Plasma),
        // the DMA-BUF path can silently fail and produce a blank webview.
        //std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus existing window on second instance
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Resolve paths
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&app_data_dir).ok();

            let config_path = paths::config_path(&app_handle);
            let lib_dir = paths::lib_dir(&app_handle);
            let log_dir = paths::log_dir(&app_handle);
            let tmp_dir = std::env::temp_dir().join("RefereeProxy");
            let _ = std::fs::remove_dir_all(&tmp_dir);
            std::fs::create_dir_all(&tmp_dir).ok();
            std::fs::create_dir_all(&lib_dir).ok();
            if let Err(e) = deps::ensure_universal_shaders(&lib_dir) {
                warn!("[Setup]: Failed to install Universal shader assets: {}", e);
            }
            std::fs::create_dir_all(&log_dir).ok();

            // Set up logging with both stdout and rolling file output.
            // File logging is essential for production builds where the console
            // window is hidden (windows_subsystem = "windows" on Windows).
            let file_appender = tracing_appender::rolling::daily(&log_dir, "referee.log");
            let (non_blocking_writer, guard) = tracing_appender::non_blocking(file_appender);
            LOG_GUARD.set(guard).ok();

            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("referee=info")),
                )
                .with(tracing_subscriber::fmt::layer())
                .with(tracing_subscriber::fmt::layer().with_writer(non_blocking_writer))
                .init();

            if let Err(error) = server::validate_http_clients() {
                error!("[Main]: {}", error);
                return Err(std::io::Error::new(std::io::ErrorKind::Other, error).into());
            }

            // Load settings
            let settings_manager = Arc::new(SettingsManager::new(&config_path));

            // Ensure the API token and stable instance identity are initialized
            // and persisted across restarts.
            let (api_token, instance_id) = {
                let mut s = settings_manager.get();
                let mut changed = false;

                let api_token = match s.api_token.clone() {
                    Some(token) => token,
                    None => {
                        let token = uuid::Uuid::new_v4().to_string();
                        s.api_token = Some(token.clone());
                        changed = true;
                        token
                    }
                };

                let instance_id = match s.instance_id.clone() {
                    Some(instance_id) => instance_id,
                    None => {
                        let instance_id = uuid::Uuid::new_v4().to_string();
                        s.instance_id = Some(instance_id.clone());
                        changed = true;
                        instance_id
                    }
                };

                if changed {
                    settings_manager.update(s);
                }

                (api_token, instance_id)
            };

            let initial_settings = settings_manager.get();

            // Shared HTTP client for all outbound requests
            let http_client = match reqwest::Client::builder()
                .user_agent("REFEREE-Upscaler")
                .build()
            {
                Ok(client) => client,
                Err(error) => {
                    error!("[Main]: Failed to build setup HTTP client: {}", error);
                    return Err(error.into());
                }
            };

            // Detect GPU
            let gpu_info = gpu::detect_compatible_gpu(&lib_dir);
            info!("[Main]: GPU detected: vendor={}", gpu_info.vendor);

            // Detect encoder capabilities upfront if the encoder binary is already present
            // (i.e. setup was completed in a prior run). The setup flow will overwrite this
            // when it runs for the first time.
            let initial_capabilities = match (&gpu_info.backend, &gpu_info.encoder_path) {
                (Some(backend), Some(encoder_path)) => {
                    let rife = gpu_info
                        .rife_worker_path
                        .as_deref()
                        .map(pipeline::detect_rife_capability)
                        .unwrap_or(false);
                    let caps = pipeline::detect_encoder_capabilities(encoder_path, backend)
                        .with_rife(rife);
                    info!(
                        "[Main]: Encoder capabilities — fruc={}, truehdr={}, rife={}",
                        caps.has_fruc, caps.has_truehdr, caps.has_rife
                    );
                    Some(caps)
                }
                _ => None,
            };

            let approved_origins_path = app_data_dir.join("approved-origins.json");
            let server_state = ServerState {
                sessions: pipeline::new_session_map(),
                instance_id,
                gpu_info: Arc::new(RwLock::new(gpu_info.clone())),
                settings: Arc::new(RwLock::new(ServerSettings::from(&initial_settings))),
                encoder_capabilities: Arc::new(RwLock::new(initial_capabilities)),
                tmp_dir: tmp_dir.clone(),
                app_handle: Some(app_handle.clone()),
                api_token: Arc::new(std::sync::RwLock::new(api_token)),
                pending_consents: Arc::new(DashMap::new()),
                approved_origins: Arc::new(load_approved_origins(&approved_origins_path)),
                approved_origins_path: Some(approved_origins_path),
                settings_path: Some(config_path.clone()),
                settings_manager: Some(settings_manager.clone()),
                rate_limit_auth: Arc::new(server::RateLimiter::new(5, 60)),
                rate_limit_stream: Arc::new(server::RateLimiter::new(3, 60)),
                pending_consent_ui: Arc::new(std::sync::Mutex::new(None)),
                setup_complete: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            };

            let setup_done = Arc::new(Notify::new());

            let app_state = AppState {
                settings_manager,
                server_state: server_state.clone(),
                lib_dir: lib_dir.clone(),
                setup_done: setup_done.clone(),
                setup_state: Arc::new(std::sync::Mutex::new(SetupLifecycle::default())),
                http_client,
            };

            app.manage(app_state);

            // Setup tray
            tray::setup_tray(&app_handle)?;

            // Show the window on first launch
            if let Ok(window) = tray::get_or_create_window(&app_handle) {
                let _ = window.show();
                let _ = window.set_focus();
            }

            // Start server and status polling in background
            let server_state_clone = server_state.clone();
            let app_handle_clone = app_handle.clone();
            let lib_dir_clone = lib_dir.clone();

            tauri::async_runtime::spawn(async move {
                // Start the HTTP server
                let server_state_for_server = server_state_clone.clone();
                tokio::spawn(async move {
                    server::start_server(server_state_for_server).await;
                });

                // GPU status refresh loop — wait for setup to complete before polling,
                // so we don't repeatedly log "Encoder not found" during the initial download.
                let gpu_arc = server_state_clone.gpu_info.clone();
                let lib_dir_for_refresh = lib_dir_clone.clone();
                tokio::spawn(async move {
                    setup_done.notified().await;
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                    loop {
                        interval.tick().await;
                        let lib_dir_spawn = lib_dir_for_refresh.clone();
                        let new_info = tokio::task::spawn_blocking(move || {
                            gpu::detect_compatible_gpu(&lib_dir_spawn)
                        })
                        .await
                        .unwrap_or_default();
                        *gpu_arc.write().await = new_info;
                    }
                });

                // Status push to renderer loop.
                // Only emits when the window is visible to avoid serialisation
                // overhead while the app is idle in the tray.
                let sessions = server_state_clone.sessions.clone();
                let gpu_info_arc = server_state_clone.gpu_info.clone();
                let settings_arc = server_state_clone.settings.clone();
                let caps_arc = server_state_clone.encoder_capabilities.clone();
                let setup_complete_arc = server_state_clone.setup_complete.clone();
                tokio::spawn(async move {
                    let mut last_status_signature: Option<String> = None;
                    loop {
                        // Use a shorter tick when active, longer when idle.
                        let window_visible = [
                            crate::tray::MAIN_WINDOW_LABEL,
                            crate::tray::STREAM_PLAYER_WINDOW_LABEL,
                        ]
                        .into_iter()
                        .any(|label| {
                            app_handle_clone
                                .get_webview_window(label)
                                .and_then(|w| w.is_visible().ok())
                                .unwrap_or(false)
                        });

                        if !window_visible {
                            last_status_signature = None;
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }

                        let has_active_sessions = !sessions.is_empty();
                        let mut gpu = gpu_info_arc.read().await.clone();
                        // Poll utilization in real-time only while a stream is active — no child
                        // processes when the app is idle in standby.
                        if has_active_sessions {
                            if let Some(util) = gpu::read_utilization(&gpu.vendor) {
                                gpu.utilization = Some(util);
                            }
                        }
                        let settings = settings_arc.read().await.clone();
                        let caps = caps_arc.read().await.clone();
                        let setup_complete =
                            setup_complete_arc.load(std::sync::atomic::Ordering::Acquire);
                        let status = server::build_status(
                            &gpu,
                            &settings,
                            &sessions,
                            caps.as_ref(),
                            setup_complete,
                        );
                        let status =
                            server::apply_status_readiness(status, &gpu, &settings, setup_complete)
                                .await;
                        let status =
                            server::enrich_status_with_remote_processing_stats(status, &sessions)
                                .await;
                        let status_signature = serde_json::to_string(&status).ok();
                        if status_signature.is_none()
                            || status_signature.as_ref() != last_status_signature.as_ref()
                        {
                            let _ = app_handle_clone.emit("status-update", &status);
                            last_status_signature = status_signature;
                        }

                        let sleep_secs = if has_active_sessions { 1 } else { 5 };
                        tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
                    }
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_settings,
            commands::get_boot_setting,
            commands::get_app_version,
            commands::get_api_token,
            commands::get_pending_consent,
            commands::respond_to_consent,
            commands::check_for_update,
            commands::download_and_install_update,
            commands::stop_stream,
            commands::get_setup_state,
            commands::retry_setup,
            commands::save_settings,
            commands::save_stream_settings,
            commands::set_boot_setting,
            commands::open_external,
            commands::open_github,
            commands::minimize_window,
            commands::close_window,
            commands::toggle_pin,
            commands::renderer_ready,
            commands::detect_players,
            commands::launch_player,
            commands::get_approved_origins,
            commands::revoke_approved_origin,
            commands::discover_lan_peers,
            commands::get_linked_relay_status,
            commands::link_relay_peer,
            commands::unlink_relay_peer,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Prevent Tauri from exiting when the last window is closed/destroyed so
            // the app stays alive in the tray. Explicit app.exit() calls carry a
            // Some(code) and must not be blocked, otherwise quit never works.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
