use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;

use crate::DesktopState;

fn map_update_error(err: impl std::fmt::Display) -> String {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("sharing violation")
        || lower.contains("used by another process")
        || lower.contains("os error 32")
    {
        return "A vsplit CLI process is currently running; wait for it to finish and retry"
            .to_string();
    }
    msg
}

#[tauri::command]
pub async fn check_update_available(app: AppHandle) -> Result<Option<String>, String> {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(update.version)),
        Ok(None) => Ok(None),
        Err(err) => {
            tracing::debug!(error = %err, "update check skipped (offline or unsigned dev build)");
            Ok(None)
        }
    }
}

#[tauri::command]
pub async fn check_and_install_update(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    let active = state.server_state.coordinator.active_count();
    if active > 0 {
        return Err(format!(
            "Cannot install update while {active} pipeline job(s) are running; cancel or wait for them to finish"
        ));
    }

    let updater = app.updater().map_err(map_update_error)?;
    let Some(update) = updater.check().await.map_err(map_update_error)? else {
        return Err("Already up to date".to_string());
    };

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(map_update_error)?;

    app.restart();
}
