use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use vsplit_core::config::load_app_config;
use vsplit_core::doctor::run_doctor;

use crate::routes::ApiError;
use crate::session::AppState;

pub async fn get_doctor(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let root = state.working_root();
    let report = tokio::task::spawn_blocking(move || {
        let app_cfg = load_app_config().unwrap_or_default();
        run_doctor(root.as_deref(), &app_cfg)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(serde_json::to_value(report).unwrap_or(json!({}))))
}
