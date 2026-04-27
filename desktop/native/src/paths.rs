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
