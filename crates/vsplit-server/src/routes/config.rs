use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use vsplit_core::config::{
    load_resolved_config, read_project_config_raw, write_project_config_raw, ConfigError,
};
use vsplit_core::schemas::config::Config;

use crate::routes::{require_working_root, ApiError};
use crate::session::AppState;

pub async fn get_config(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let cfg =
        load_resolved_config(&root, None).map_err(|e| ApiError::unprocessable(e.to_string()))?;
    Ok(Json(serde_json::to_value(cfg).unwrap_or(json!({}))))
}

pub async fn put_config(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let cfg: Config = serde_json::from_value(payload)
        .map_err(|e| ApiError::unprocessable(format!("invalid config: {e}")))?;
    cfg.validate()
        .map_err(|e| ApiError::unprocessable(e.to_string()))?;
    let yaml_str = serde_yml::to_string(&cfg)
        .map_err(|e| ApiError::internal(format!("yaml serialize: {e}")))?;
    write_project_config_raw(&root, &yaml_str)
        .map_err(|e| ApiError::unprocessable(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn get_config_raw(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let text = read_project_config_raw(&root).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({ "text": text })))
}

#[derive(Deserialize)]
pub struct RawConfigBody {
    pub text: String,
}

pub async fn put_config_raw(
    State(state): State<AppState>,
    Json(body): Json<RawConfigBody>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    match write_project_config_raw(&root, &body.text) {
        Ok(_) => Ok(Json(json!({ "ok": true }))),
        Err(ConfigError::NotMapping(_)) => {
            Err(ApiError::bad_request("YAML root must be a mapping"))
        }
        Err(ConfigError::Yaml { source, .. }) => Err(ApiError::unprocessable(source.to_string())),
        Err(ConfigError::Schema(e)) => Err(ApiError::unprocessable(e.to_string())),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}
