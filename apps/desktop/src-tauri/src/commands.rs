use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use vsplit_core::config::{load_app_config, save_app_config};
use vsplit_core::schemas::app_config::AppConfig;
use vsplit_core::toolchain::refresh_toolchain;

use crate::cli_shim;
use crate::DesktopState;

#[derive(Serialize)]
pub struct BootstrapPayload {
    pub base_url: String,
    pub token: String,
    pub boot_ms: u128,
    pub os: &'static str,
}

#[tauri::command]
pub fn get_server_bootstrap(state: State<'_, DesktopState>) -> BootstrapPayload {
    BootstrapPayload {
        base_url: state.bootstrap.base_url.clone(),
        token: state.bootstrap.token.clone(),
        boot_ms: state.boot_ms,
        os: std::env::consts::OS,
    }
}

#[tauri::command]
pub fn get_app_config() -> Result<AppConfig, String> {
    load_app_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_app_config(config: AppConfig) -> Result<AppConfig, String> {
    save_app_config(&config).map_err(|e| e.to_string())?;
    // Refresh session MediaToolchain cache immediately so newly picked ffmpeg/ffprobe paths apply.
    let _ = refresh_toolchain(&config);
    Ok(config)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Select vsplit video folder")
            .blocking_pick_folder()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_file(app: AppHandle, title: Option<String>) -> Result<Option<String>, String> {
    let dialog_title = title.unwrap_or_else(|| "Select file".to_string());
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(&dialog_title)
            .blocking_pick_file()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn repair_cli_shim(app: AppHandle) -> Result<String, String> {
    cli_shim::sync_if_needed(&app).map_err(|e| e.to_string())
}
