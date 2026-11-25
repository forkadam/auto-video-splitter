use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use axum::extract::{Path as AxumPath, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use vsplit_core::actions::{cuts_get, cuts_set};
use vsplit_core::index::load_index;
use vsplit_core::keyframes::load_keyframes;
use vsplit_core::locks::{inspect_workspace_lock, LockActor};
use vsplit_core::manifest::load_manifest;
use vsplit_core::paths::{validate_slug, workspace_dir};
use vsplit_core::process::apply_no_window_std;
use vsplit_core::schemas::cuts::CutsJson;
use vsplit_core::schemas::manifest::{Manifest, StageStatus};
use vsplit_core::schemas::scenes::ScenesJson;

use crate::routes::{require_working_root, ApiError};
use crate::session::AppState;

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v"];

pub fn require_indexed_slug(root: &Path, slug: &str) -> Result<(), ApiError> {
    validate_slug(slug).map_err(|_| ApiError::not_found(format!("workspace not found: {slug}")))?;
    let idx = load_index(root).map_err(|e| ApiError::internal(e.to_string()))?;
    if !idx.workspaces.contains_key(slug) {
        return Err(ApiError::not_found(format!("workspace not found: {slug}")));
    }
    Ok(())
}

fn list_thumbs(root: &Path, slug: &str) -> Vec<String> {
    let img_dir = workspace_dir(root, slug).join("preview").join("images");
    let Ok(entries) = fs::read_dir(img_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.is_file()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("jpg"))
                    .unwrap_or(false)
            {
                Some(e.file_name().to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

fn count_outputs(root: &Path, slug: &str) -> usize {
    let out_dir = workspace_dir(root, slug).join("output");
    let Ok(entries) = fs::read_dir(out_dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let p = e.path();
            p.is_file()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .map(|ext| {
                        let lower = ext.to_ascii_lowercase();
                        VIDEO_EXTS.contains(&lower.as_str())
                    })
                    .unwrap_or(false)
        })
        .count()
}

fn scene_thumbs(root: &Path, slug: &str, scene_ids: &[usize]) -> BTreeMap<String, String> {
    let all = list_thumbs(root, slug);
    let mut out = BTreeMap::new();
    for &sid in scene_ids {
        let prefix = format!("scene-{sid:03}-");
        if let Some(found) = all.iter().find(|name| name.starts_with(&prefix)) {
            out.insert(sid.to_string(), found.clone());
        }
    }
    out
}

fn build_summary(root: &Path, slug: &str, m: &Manifest) -> Value {
    let statuses: Vec<StageStatus> = m.stages.values().map(|b| b.status).collect();
    let thumbs = list_thumbs(root, slug);
    let detect = m.stage("detect");
    let scenes_found = if detect.status == StageStatus::Done {
        detect.artifacts.get("scenes_found").and_then(Value::as_u64)
    } else {
        None
    };
    let stages: BTreeMap<&String, &'static str> = m
        .stages
        .iter()
        .map(|(k, b)| (k, b.status.as_str()))
        .collect();
    let locked_by = inspect_workspace_lock(root, slug);

    json!({
        "slug": slug,
        "ignored": m.ignored,
        "duration_seconds": m.metadata.duration_seconds,
        "source": m.source.relative_path,
        "stages": stages,
        "any_failed": statuses.contains(&StageStatus::Failed),
        "any_awaiting": statuses.contains(&StageStatus::AwaitingUser),
        "all_done": statuses.iter().all(|&s| s == StageStatus::Done),
        "first_thumb": thumbs.first(),
        "scenes_found": scenes_found,
        "output_count": count_outputs(root, slug),
        "locked_by": locked_by,
    })
}

pub async fn list_workspaces(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(root) = state.working_root() else {
        return Ok(Json(json!({ "workspaces": [] })));
    };
    let idx = load_index(&root).map_err(|e| ApiError::internal(e.to_string()))?;
    let mut items = Vec::with_capacity(idx.workspaces.len());
    for slug in idx.workspaces.keys() {
        let ws = workspace_dir(&root, slug);
        if let Ok(m) = load_manifest(&ws) {
            items.push(build_summary(&root, slug, &m));
        }
    }
    Ok(Json(json!({ "workspaces": items })))
}

pub async fn get_workspace(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let ws = workspace_dir(&root, &slug);
    let m = load_manifest(&ws)
        .map_err(|_| ApiError::not_found(format!("workspace not found: {slug}")))?;
    let mut obj = build_summary(&root, &slug, &m);
    if let Value::Object(ref mut map) = obj {
        map.insert("workspace_path".into(), json!(ws.display().to_string()));
        map.insert("thumb_names".into(), json!(list_thumbs(&root, &slug)));
        map.insert(
            "manifest".into(),
            serde_json::to_value(&m).unwrap_or(Value::Null),
        );
    }
    Ok(Json(obj))
}

pub async fn get_cuts(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let doc = cuts_get(&root, &slug).map_err(ApiError::from_action)?;

    let ws = workspace_dir(&root, &slug);
    let scene_ids: Vec<usize> = doc.cuts.scenes.iter().map(|s| s.id).collect();
    let kf_path = ws.join("probe").join("keyframes.csv");
    let mut keyframes = if kf_path.is_file() {
        load_keyframes(&kf_path).unwrap_or_default()
    } else {
        Vec::new()
    };
    keyframes.sort_by(|a, b| a.total_cmp(b));

    let scenes_path = ws.join("detect").join("scenes.json");
    let detected_count = fs::read_to_string(&scenes_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<ScenesJson>(&raw).ok())
        .map(|s| s.scenes.len())
        .unwrap_or(0);

    let duration_seconds = load_manifest(&ws)
        .ok()
        .and_then(|m| m.metadata.duration_seconds);

    Ok(Json(json!({
        "cuts": doc.cuts,
        "hash": doc.hash,
        "scene_thumbs": scene_thumbs(&root, &slug, &scene_ids),
        "keyframes": keyframes,
        "detected_count": detected_count,
        "duration_seconds": duration_seconds,
    })))
}

#[derive(Deserialize)]
pub struct CutsPutBody {
    pub cuts: Value,
    #[serde(default)]
    pub expected_hash: Option<String>,
}

pub async fn put_cuts(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
    Json(body): Json<CutsPutBody>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let parsed: CutsJson = serde_json::from_value(body.cuts)
        .map_err(|e| ApiError::unprocessable(format!("invalid cuts payload: {e}")))?;

    let updated = cuts_set(
        &root,
        &slug,
        &parsed,
        body.expected_hash.as_deref(),
        LockActor::Desktop,
    )
    .map_err(ApiError::from_action)?;

    Ok(Json(json!({ "hash": updated.hash })))
}

pub async fn open_preview(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let preview_html = workspace_dir(&root, &slug)
        .join("preview")
        .join("index.html");
    if !preview_html.is_file() {
        return Err(ApiError::not_found("preview not generated yet"));
    }

    let target = preview_html.display().to_string();
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &target]);
        apply_no_window_std(&mut cmd);
        cmd.spawn()
            .map_err(|e| ApiError::internal(format!("failed to open preview: {e}")))?;
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(&target);
        apply_no_window_std(&mut cmd);
        cmd.spawn()
            .map_err(|e| ApiError::internal(format!("failed to open preview: {e}")))?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(&target);
        apply_no_window_std(&mut cmd);
        cmd.spawn()
            .map_err(|e| ApiError::internal(format!("failed to open preview: {e}")))?;
    }

    Ok(Json(json!({ "opened": true, "path": target })))
}
