use crate::commands::AppState;
use crate::settings::Settings;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder},
    window::Color,
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

pub const MAIN_WINDOW_LABEL: &str = "main";
pub const STREAM_PLAYER_WINDOW_LABEL: &str = "stream-player";

/// Returns the existing main window, or creates it on first call.
/// The window starts hidden; callers are responsible for showing it.
pub fn get_or_create_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        return Ok(window);
    }

    let settings = {
        let state = app.state::<AppState>();
        state.settings_manager.get()
    };

    let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::App("/".into()))
        .title("REFEREE Upscaler")
        .inner_size(300.0, 500.0)
        .resizable(false)
        .decorations(false)
        .visible(false)
        .background_color(Color(0x12, 0x12, 0x12, 0xFF));

    // Set the window icon explicitly on Linux (taskbar/dock icon is not inherited automatically).
    #[cfg(target_os = "linux")]
    let builder = builder.icon(app.default_window_icon().cloned().unwrap())?;

    // Cap V8's old-generation heap on Windows (WebView2 only) to reduce idle memory.
    #[cfg(target_os = "windows")]
    let builder =
        builder.additional_browser_args("--js-flags=--max-old-space-size=64 --disable-extensions");

    let window = builder.build()?;

    if settings.always_on_top {
        let _ = window.set_always_on_top(true);
    }

    Ok(window)
}

/// Returns the existing stream-player window, or creates it on first call.
pub fn get_or_create_stream_player_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(STREAM_PLAYER_WINDOW_LABEL) {
        return Ok(window);
    }

    let builder = WebviewWindowBuilder::new(
        app,
        STREAM_PLAYER_WINDOW_LABEL,
        WebviewUrl::App("player.html".into()),
    )
    .title("REFEREE Stream Player")
    .inner_size(1280.0, 720.0)
    .min_inner_size(640.0, 360.0)
    .resizable(true)
    .visible(false)
    .background_color(Color(0x08, 0x0B, 0x11, 0xFF));

    #[cfg(target_os = "linux")]
    let builder = builder.icon(app.default_window_icon().cloned().unwrap())?;

    #[cfg(target_os = "windows")]
    let builder =
        builder.additional_browser_args("--js-flags=--max-old-space-size=64 --disable-extensions");

    builder.build()
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = {
        let state = app.state::<AppState>();
        state.settings_manager.get()
    };

    build_tray(app, &settings)
}

fn build_tray(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let open_settings =
        MenuItem::with_id(app, "open_settings", "Open Settings", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let always_on_top = CheckMenuItem::with_id(
        app,
        "always_on_top",
        "Always on Top",
        true,
        settings.always_on_top,
        None::<&str>,
    )?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let minimize_to_tray = CheckMenuItem::with_id(
        app,
        "minimize_to_tray",
        "Minimize to Tray",
        true,
        settings.minimize_to_tray,
        None::<&str>,
    )?;
    let close_to_tray = CheckMenuItem::with_id(
        app,
        "close_to_tray",
        "Close to Tray",
        true,
        settings.close_to_tray,
        None::<&str>,
    )?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_settings,
            &sep1,
            &always_on_top,
            &sep2,
            &minimize_to_tray,
            &close_to_tray,
            &sep3,
            &quit,
        ],
    )?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().unwrap())
        .tooltip("REFEREE Upscaler")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "open_settings" => {
                    if let Ok(window) = get_or_create_window(&app) {
                        let _ = window.show();
                        let _ = window.set_focus();
                        let _ = app.emit("navigate-view", "settings");
                    }
                }
                "always_on_top" => {
                    let state = app.state::<AppState>();
                    let mut settings = state.settings_manager.get();
                    settings.always_on_top = !settings.always_on_top;
                    state.settings_manager.update(settings.clone());
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_always_on_top(settings.always_on_top);
                    }
                    let _ = app.emit("settings-sync", &settings);
                }
                "minimize_to_tray" => {
                    let state = app.state::<AppState>();
                    let mut settings = state.settings_manager.get();
                    settings.minimize_to_tray = !settings.minimize_to_tray;
                    state.settings_manager.update(settings.clone());
                    let _ = app.emit("settings-sync", &settings);
                }
                "close_to_tray" => {
                    let state = app.state::<AppState>();
                    let mut settings = state.settings_manager.get();
                    settings.close_to_tray = !settings.close_to_tray;
                    state.settings_manager.update(settings.clone());
                    let _ = app.emit("settings-sync", &settings);
                }
                "quit" => {
                    let sessions = {
                        let state = app.state::<AppState>();
                        state.server_state.sessions.clone()
                    };
                    let app_clone = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::server::shutdown(&sessions).await;
                        app_clone.exit(0);
                    });
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                        return;
                    }
                }
                // Window is hidden or doesn't exist yet — create and show it.
                if let Ok(window) = get_or_create_window(app) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
