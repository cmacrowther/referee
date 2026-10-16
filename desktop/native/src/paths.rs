use std::path::PathBuf;
use tauri::Manager;

pub fn lib_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");
    app_data.join("lib")
}

pub fn config_path(app_handle: &tauri::AppHandle) -> PathBuf {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");
    app_data.join("config.json")
}

pub fn log_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");
    app_data.join("logs")
}

/// Downloaded update installers. Private to the user, unlike the shared
/// temporary folder on Linux, because the installer runs elevated.
pub fn update_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_cache_dir()
        .expect("failed to resolve app cache dir")
        .join("updates")
}
