use std::collections::BTreeMap;
use std::path::PathBuf;

use axum::extract::{Path as AxumPath, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use vsplit_core::actions::{
    approve_workspaces, clean_workspaces, ignore_workspaces, reset_workspaces, unignore_workspaces,
};
use vsplit_core::collect::collect_outputs;
use vsplit_core::config::load_resolved_config;
use vsplit_core::locks::{LockActor, LockError};
use vsplit_core::orchestrator::{dispatch, resolve_stages_to_run};
use vsplit_core::rename::{apply_rename_plan, plan_renames, RenameError, RenameOptions};
use vsplit_core::schemas::config::{CollectMode, VALID_CONCURRENCY_STAGES};
use vsplit_core::toolchain::cached_toolchain;

use crate::routes::workspaces::require_indexed_slug;
use crate::routes::{require_working_root, ApiError};
use crate::session::AppState;

#[derive(Deserialize, Default)]
pub struct RunBody {
    pub only: Option<String>,
    pub from: Option<String>,
    pub through: Option<String>,
    pub concurrency_overrides: Option<BTreeMap<String, usize>>,
}

#[derive(Deserialize)]
pub struct ResetBody {
    pub from: String,
}

#[derive(Deserialize)]
pub struct BatchSlugs {
    pub slugs: Vec<String>,
}

#[derive(Deserialize)]
pub struct BatchReset {
    pub slugs: Vec<String>,
    pub from: String,
}

#[derive(Deserialize)]
pub struct BatchRun {
    pub slugs: Vec<String>,
    pub only: Option<String>,
    pub from: Option<String>,
    pub through: Option<String>,
    pub concurrency_overrides: Option<BTreeMap<String, usize>>,
}

#[derive(Deserialize, Default)]
pub struct CollectBody {
    pub slugs: Option<Vec<String>>,
    pub dest: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Deserialize, Default)]
pub struct RenameBody {
    pub glob: Option<String>,
    pub recursive: Option<bool>,
    pub format: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

fn map_rename_error(err: RenameError) -> ApiError {
    match err {
        RenameError::InvalidFormat { .. }
        | RenameError::InvalidGlob { .. }
        | RenameError::NotADirectory(_) => ApiError::unprocessable(err.to_string()),
        RenameError::Lock(LockError::Held { slug, lease }) => ApiError {
            status: axum::http::StatusCode::CONFLICT,
            body: json!({
                "detail": format!(
                    "workspace {slug:?} is locked by {} (pid {}, stage {})",
                    lease.actor.as_str(),
                    lease.pid,
                    lease.stage
                ),
                "locked_by": lease,
            }),
        },
        RenameError::Toolchain(e) => {
            ApiError::new(axum::http::StatusCode::SERVICE_UNAVAILABLE, e.to_string())
        }
        other => ApiError::internal(other.to_string()),
    }
}

pub async fn rename_videos_route(
    State(state): State<AppState>,
    body: Option<Json<RenameBody>>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let req = body.map(|b| b.0).unwrap_or_default();
    let cfg =
        load_resolved_config(&root, None).map_err(|e| ApiError::unprocessable(e.to_string()))?;
    let toolchain = cached_toolchain();

    let glob = req
        .glob
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty());

    let opts = RenameOptions {
        recursive: req.recursive,
        glob,
        format: req.format,
        dry_run: req.dry_run,
    };

    let plan = plan_renames(&root, &cfg, &toolchain, &opts)
        .await
        .map_err(map_rename_error)?;

    if req.dry_run {
        let report = plan.into_dry_run_report();
        return Ok(Json(
            serde_json::to_value(report).map_err(|e| ApiError::internal(e.to_string()))?,
        ));
    }

    let affected_slugs = plan.affected_slugs();
    let watch_guard = state.suspend_workspace_watches(&root, &affected_slugs);

    let root_for_apply = root.clone();
    let apply_res = tokio::task::spawn_blocking(move || {
        let _guard = watch_guard;
        apply_rename_plan(&root_for_apply, plan, LockActor::Desktop)
    })
    .await;

    let report = apply_res
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(map_rename_error)?;

    Ok(Json(
        serde_json::to_value(report).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}

pub async fn clean_one(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let slug_vec = vec![slug];
    let res =
        tokio::task::spawn_blocking(move || clean_workspaces(&root, &slug_vec, LockActor::Desktop))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "cleaned": res.removed.len() })))
}

pub async fn approve_one(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let slug_vec = vec![slug];
    let res = tokio::task::spawn_blocking(move || {
        approve_workspaces(&root, &slug_vec, LockActor::Desktop)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "approved": res.len() })))
}

pub async fn ignore_one(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let slug_vec = vec![slug];
    let res = tokio::task::spawn_blocking(move || {
        ignore_workspaces(&root, &slug_vec, LockActor::Desktop)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "ignored": res.len() })))
}

/// Fixed response key from legacy `{"ignored": n}` to `{"unignored": n}` per design doc §4.
pub async fn unignore_one(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let slug_vec = vec![slug];
    let res = tokio::task::spawn_blocking(move || {
        unignore_workspaces(&root, &slug_vec, LockActor::Desktop)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "unignored": res.len() })))
}

pub async fn reset_one(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
    Json(body): Json<ResetBody>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let slug_vec = vec![slug];
    let from = body.from;
    tokio::task::spawn_blocking(move || {
        reset_workspaces(&root, &slug_vec, &from, LockActor::Desktop)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "ok": true })))
}

async fn run_slugs_impl(
    state: &AppState,
    slugs: Vec<String>,
    only: Option<String>,
    from: Option<String>,
    through: Option<String>,
    concurrency_overrides: Option<BTreeMap<String, usize>>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(state)?;
    let mut cfg =
        load_resolved_config(&root, None).map_err(|e| ApiError::unprocessable(e.to_string()))?;
    if let Some(ov) = concurrency_overrides {
        for (k, v) in ov {
            if !VALID_CONCURRENCY_STAGES.contains(&k.as_str()) || v < 1 {
                return Err(ApiError::unprocessable(format!(
                    "invalid concurrency override: {k}={v}"
                )));
            }
            cfg.concurrency.insert(k, v);
        }
    }
    let stages = resolve_stages_to_run(only.as_deref(), from.as_deref(), through.as_deref())
        .map_err(ApiError::unprocessable)?;
    let toolchain = cached_toolchain();
    let summary = dispatch(
        &root,
        &slugs,
        &cfg,
        &toolchain,
        &stages,
        &state.coordinator,
        LockActor::Desktop,
    )
    .await;

    state.sync_watches();

    if summary.any_locked {
        return Err(ApiError {
            status: axum::http::StatusCode::CONFLICT,
            body: json!({
                "detail": "workspace is locked by another process",
                "summary": summary,
            }),
        });
    }

    Ok(Json(json!({ "ok": true, "summary": summary })))
}

pub async fn run_one(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
    body: Option<Json<RunBody>>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    require_indexed_slug(&root, &slug)?;
    let req = body.map(|b| b.0).unwrap_or_default();
    run_slugs_impl(
        &state,
        vec![slug],
        req.only,
        req.from,
        req.through,
        req.concurrency_overrides,
    )
    .await
}

pub async fn batch_clean(
    State(state): State<AppState>,
    Json(body): Json<BatchSlugs>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let slugs = body.slugs;
    let res =
        tokio::task::spawn_blocking(move || clean_workspaces(&root, &slugs, LockActor::Desktop))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "cleaned": res.removed.len() })))
}

pub async fn batch_approve(
    State(state): State<AppState>,
    Json(body): Json<BatchSlugs>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let slugs = body.slugs;
    let res =
        tokio::task::spawn_blocking(move || approve_workspaces(&root, &slugs, LockActor::Desktop))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "approved": res.len() })))
}

pub async fn batch_reset(
    State(state): State<AppState>,
    Json(body): Json<BatchReset>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let slugs = body.slugs;
    let from = body.from;
    tokio::task::spawn_blocking(move || reset_workspaces(&root, &slugs, &from, LockActor::Desktop))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(ApiError::from_action)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn batch_run(
    State(state): State<AppState>,
    Json(body): Json<BatchRun>,
) -> Result<Json<Value>, ApiError> {
    run_slugs_impl(
        &state,
        body.slugs,
        body.only,
        body.from,
        body.through,
        body.concurrency_overrides,
    )
    .await
}

pub async fn batch_collect(
    State(state): State<AppState>,
    body: Option<Json<CollectBody>>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let req = body.map(|b| b.0).unwrap_or_default();
    let cfg =
        load_resolved_config(&root, None).map_err(|e| ApiError::unprocessable(e.to_string()))?;

    let dest_path = match req.dest {
        Some(ref d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => {
            let configured = PathBuf::from(&cfg.collect.destination);
            if configured.is_absolute() {
                configured
            } else {
                root.join(configured)
            }
        }
    };

    let mode = match req.mode.as_deref() {
        None => cfg.collect.mode,
        Some("copy") => CollectMode::Copy,
        Some("move") => CollectMode::Move,
        Some("link") => CollectMode::Link,
        Some(other) => return Err(ApiError::unprocessable(format!("unknown mode: {other:?}"))),
    };

    let slugs = req.slugs;
    let force = req.force;
    let dry_run = req.dry_run;
    let report = tokio::task::spawn_blocking(move || {
        collect_outputs(
            &root,
            slugs.as_deref(),
            Some(&dest_path),
            mode,
            force,
            dry_run,
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(json!({
        "collected": report.collected,
        "from_workspaces": report.from_workspaces,
        "skipped_not_done": report.skipped_not_done,
        "skipped_no_output": report.skipped_no_output,
        "skipped_existing": report.skipped_existing,
        "skipped_ignored": report.skipped_ignored,
        "destination": report.destination,
        "mode": report.mode.as_str(),
        "dry_run": dry_run,
        "files": report.files,
    })))
}
