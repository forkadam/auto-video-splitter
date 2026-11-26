use std::path::PathBuf;

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use vsplit_core::config::{load_app_config, load_resolved_config, save_app_config};
use vsplit_core::scan::{adopt_or_find, scan_folder, RelocatePolicy, ScanOptions};

use crate::routes::{require_working_root, ApiError};
use crate::session::{AppState, OpenProjectError};

#[derive(Deserialize)]
pub struct OpenProjectBody {
    pub path: String,
}

pub async fn open_project(
    State(state): State<AppState>,
    Json(body): Json<OpenProjectBody>,
) -> Result<Json<Value>, ApiError> {
    let target = PathBuf::from(body.path.trim());
    match state.open_project(target, true) {
        Ok((root, recent)) => Ok(Json(json!({
            "working_root": root.display().to_string(),
            "recent": recent,
        }))),
        Err(OpenProjectError::ActiveJobs(n)) => Err(ApiError::conflict(format!(
            "cannot switch folder while {n} job(s) are running; cancel active jobs first"
        ))),
        Err(OpenProjectError::InvalidPath(msg)) => Err(ApiError::bad_request(msg)),
    }
}

pub async fn recent_projects(State(state): State<AppState>) -> Json<Value> {
    let mut cfg = load_app_config().unwrap_or_default();
    if cfg.prune_missing_recent_folders() {
        let _ = save_app_config(&cfg);
    }
    let recent: Vec<String> = cfg
        .recent_folders
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let working_root = state.working_root().map(|p| p.display().to_string());
    Json(json!({
        "working_root": working_root,
        "recent": recent,
    }))
}

#[derive(Deserialize, Default)]
pub struct RemoveRecentBody {
    pub path: Option<String>,
    #[serde(default)]
    pub clear_all: bool,
}

pub async fn remove_recent_project(
    State(state): State<AppState>,
    Json(body): Json<RemoveRecentBody>,
) -> Result<Json<Value>, ApiError> {
    let mut cfg = load_app_config().unwrap_or_default();
    cfg.prune_missing_recent_folders();
    if body.clear_all {
        cfg.clear_recent_folders();
    } else if let Some(ref p) = body.path {
        cfg.remove_recent_folder(&PathBuf::from(p));
    }
    save_app_config(&cfg).map_err(|e| ApiError::internal(e.to_string()))?;

    let recent: Vec<String> = cfg
        .recent_folders
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let working_root = state.working_root().map(|p| p.display().to_string());
    Ok(Json(json!({
        "working_root": working_root,
        "recent": recent,
    })))
}

#[derive(Deserialize)]
pub struct AdoptFileBody {
    pub path: String,
}

/// Single-file `one`-style adoption for desktop drag-and-drop: switches to the file's
/// parent directory if needed and adopts just that video without scanning sibling files.
pub async fn adopt_file(
    State(state): State<AppState>,
    Json(body): Json<AdoptFileBody>,
) -> Result<Json<Value>, ApiError> {
    let file_path = PathBuf::from(body.path.trim());
    if !file_path.is_file() {
        return Err(ApiError::bad_request(format!(
            "file does not exist: {}",
            file_path.display()
        )));
    }
    let canon = file_path.canonicalize().unwrap_or(file_path);
    let parent = canon
        .parent()
        .ok_or_else(|| ApiError::bad_request("file has no parent directory"))?
        .to_path_buf();

    let (root, recent) = match state.open_project(parent, true) {
        Ok(pair) => pair,
        Err(OpenProjectError::ActiveJobs(n)) => {
            return Err(ApiError::conflict(format!(
                "cannot adopt file in another folder while {n} job(s) are running"
            )));
        }
        Err(OpenProjectError::InvalidPath(msg)) => return Err(ApiError::bad_request(msg)),
    };

    let root_cl = root.clone();
    let canon_cl = canon.clone();
    let slug = tokio::task::spawn_blocking(move || adopt_or_find(&root_cl, &canon_cl))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    state.sync_watches();
    Ok(Json(json!({
        "slug": slug,
        "working_root": root.display().to_string(),
        "recent": recent,
    })))
}

#[derive(Deserialize, Default)]
pub struct ScanBody {
    pub path: Option<String>,
    pub recursive: Option<bool>,
    pub relocate_policy: Option<RelocatePolicy>,
    #[serde(default)]
    pub dry_run: bool,
}

pub async fn scan(
    State(state): State<AppState>,
    body: Option<Json<ScanBody>>,
) -> Result<Json<Value>, ApiError> {
    let req = body.map(|b| b.0).unwrap_or_default();
    let root = match req.path {
        Some(ref p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => require_working_root(&state)?,
    };
    if !root.is_dir() {
        return Err(ApiError::bad_request(format!(
            "directory does not exist: {}",
            root.display()
        )));
    }

    let opts = ScanOptions {
        recursive: req.recursive,
        relocate_policy: req.relocate_policy.unwrap_or(RelocatePolicy::Auto),
        dry_run: req.dry_run,
    };

    let root_cl = root.clone();
    let report = tokio::task::spawn_blocking(move || {
        let cfg = load_resolved_config(&root_cl, None)
            .map_err(|e| ApiError::unprocessable(e.to_string()))?;
        scan_folder(&root_cl, &cfg, &opts).map_err(|e| ApiError::internal(e.to_string()))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))??;

    state.sync_watches();
    Ok(Json(serde_json::to_value(report).unwrap_or(json!({}))))
}

#[derive(Deserialize, Default)]
pub struct CancelJobsBody {
    pub slug: Option<String>,
}

pub async fn cancel_jobs(
    State(state): State<AppState>,
    body: Option<Json<CancelJobsBody>>,
) -> Json<Value> {
    let slug = body.and_then(|b| b.0.slug);
    let cancelled = state.coordinator.cancel(slug.as_deref());
    Json(json!({ "cancelled": cancelled }))
}

pub async fn active_jobs(State(state): State<AppState>) -> Json<Value> {
    let active_slugs = state.coordinator.active_slugs();
    let count = active_slugs.len();
    Json(json!({
        "active_slugs": active_slugs,
        "count": count,
    }))
}
