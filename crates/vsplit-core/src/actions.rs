use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

use crate::atomic::{atomic_write_json, robust_remove_dir_all};
use crate::hash::{bytes_sha256_hex, normalize_hash_hex};
use crate::index::{load_index, save_index, IndexError};
use crate::locks::{lock_index, try_acquire_workspace_lock, LockActor, LockError, LockLease};
use crate::manifest::{load_manifest, save_manifest, ManifestError};
use crate::paths::{safe_workspace_dir, workspace_dir, PathError};
use crate::schemas::cuts::CutsJson;
use crate::schemas::manifest::{StageBlock, StageStatus, STAGE_NAMES};
use crate::schemas::scenes::ScenesJson;
use crate::schemas::SchemaError;
use crate::stages::review::validate_ids;
use crate::stages::StageKind;

#[derive(Debug, Error)]
pub enum ActionError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error("unknown stage: {0:?}")]
    UnknownStage(String),
    #[error("cuts.json not found for workspace {0:?}")]
    CutsNotFound(String),
    #[error("log file not found for {slug}/{stage}")]
    LogNotFound { slug: String, stage: String },
    #[error("cuts.json was modified externally (expected hash {expected}, actual {actual})")]
    HashMismatch { expected: String, actual: String },
    #[error("{0}")]
    Validation(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanResult {
    pub removed: Vec<String>,
    pub kept_in_use: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CutsDocument {
    pub slug: String,
    pub hash: String,
    pub cuts: CutsJson,
}

/// Delete each named workspace directory and retire its slug.
/// Canonical lock order: acquires `index.lock` BEFORE `<slug>.lock` to prevent deadlocks.
pub fn clean_workspaces(
    working_root: &Path,
    slugs: &[String],
    actor: LockActor,
) -> Result<CleanResult, ActionError> {
    let _index_guard = lock_index(working_root)?;
    let mut idx = load_index(working_root)?;
    let mut removed = Vec::new();
    let mut kept_in_use = Vec::new();

    for slug in slugs {
        if !idx.workspaces.contains_key(slug) {
            continue;
        }
        let ws_lock = try_acquire_workspace_lock(
            working_root,
            slug,
            LockLease::new(actor, "clean", "clean"),
        )?;
        let ws = workspace_dir(working_root, slug);
        let deleted = robust_remove_dir_all(&ws);
        drop(ws_lock);
        if !deleted {
            kept_in_use.push(slug.clone());
            continue;
        }
        idx.workspaces.remove(slug);
        if !idx.retired_slugs.contains(slug) {
            idx.retired_slugs.push(slug.clone());
        }
        removed.push(slug.clone());
    }

    save_index(working_root, &mut idx)?;
    Ok(CleanResult {
        removed,
        kept_in_use,
    })
}

/// Mark each workspace's review stage approved if currently `awaiting_user`.
pub fn approve_workspaces(
    working_root: &Path,
    slugs: &[String],
    actor: LockActor,
) -> Result<Vec<String>, ActionError> {
    let now = Utc::now().to_rfc3339();
    let mut approved = Vec::new();
    for slug in slugs {
        let ws = safe_workspace_dir(working_root, slug)?;
        let _lock = try_acquire_workspace_lock(
            working_root,
            slug,
            LockLease::new(actor, "approve", "approve"),
        )?;
        let mut m = match load_manifest(&ws) {
            Ok(m) => m,
            Err(ManifestError::NotFound(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let block = m.stage_mut("review");
        if block.status != StageStatus::AwaitingUser {
            continue;
        }
        block.artifacts.insert("approved_at".into(), json!(now));
        save_manifest(&ws, &mut m)?;
        approved.push(slug.clone());
    }
    Ok(approved)
}

pub fn ignore_workspaces(
    working_root: &Path,
    slugs: &[String],
    actor: LockActor,
) -> Result<Vec<String>, ActionError> {
    let mut changed = Vec::new();
    for slug in slugs {
        let ws = safe_workspace_dir(working_root, slug)?;
        let _lock = try_acquire_workspace_lock(
            working_root,
            slug,
            LockLease::new(actor, "ignore", "ignore"),
        )?;
        let mut m = match load_manifest(&ws) {
            Ok(m) => m,
            Err(ManifestError::NotFound(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        if m.ignored {
            continue;
        }
        m.ignored = true;
        m.ignored_at = Some(Utc::now());
        save_manifest(&ws, &mut m)?;
        changed.push(slug.clone());
    }
    Ok(changed)
}

pub fn unignore_workspaces(
    working_root: &Path,
    slugs: &[String],
    actor: LockActor,
) -> Result<Vec<String>, ActionError> {
    let mut changed = Vec::new();
    for slug in slugs {
        let ws = safe_workspace_dir(working_root, slug)?;
        let _lock = try_acquire_workspace_lock(
            working_root,
            slug,
            LockLease::new(actor, "unignore", "unignore"),
        )?;
        let mut m = match load_manifest(&ws) {
            Ok(m) => m,
            Err(ManifestError::NotFound(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        if !m.ignored {
            continue;
        }
        m.ignored = false;
        m.ignored_at = None;
        save_manifest(&ws, &mut m)?;
        changed.push(slug.clone());
    }
    Ok(changed)
}

pub fn reset_workspaces(
    working_root: &Path,
    slugs: &[String],
    from_stage: &str,
    actor: LockActor,
) -> Result<Vec<String>, ActionError> {
    let Some(start) = STAGE_NAMES.iter().position(|&n| n == from_stage) else {
        return Err(ActionError::UnknownStage(from_stage.to_string()));
    };
    let targets = &STAGE_NAMES[start..];
    let mut reset = Vec::new();

    for slug in slugs {
        let ws = safe_workspace_dir(working_root, slug)?;
        let _lock = try_acquire_workspace_lock(
            working_root,
            slug,
            LockLease::new(actor, format!("reset:{from_stage}"), "reset"),
        )?;
        let mut m = match load_manifest(&ws) {
            Ok(m) => m,
            Err(ManifestError::NotFound(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        for &stage_name in targets {
            let kind = StageKind::parse(stage_name).unwrap();
            if let Some(sub) = kind.output_subdir() {
                let _ = robust_remove_dir_all(&ws.join(sub));
            }
            m.stages
                .insert(stage_name.to_string(), StageBlock::default());
        }
        save_manifest(&ws, &mut m)?;
        reset.push(slug.clone());
    }

    Ok(reset)
}

pub fn cuts_get(working_root: &Path, slug: &str) -> Result<CutsDocument, ActionError> {
    let ws = safe_workspace_dir(working_root, slug)?;
    let cuts_path = ws.join("decisions").join("cuts.json");
    if !cuts_path.is_file() {
        return Err(ActionError::CutsNotFound(slug.to_string()));
    }
    let bytes = fs::read(&cuts_path)?;
    let hash = bytes_sha256_hex(&bytes);
    let cuts: CutsJson = serde_json::from_slice(&bytes)?;
    cuts.validate()?;
    Ok(CutsDocument {
        slug: slug.to_string(),
        hash,
        cuts,
    })
}

/// Validate and atomically write `decisions/cuts.json` under exclusive `<slug>.lock`,
/// enforcing optional optimistic concurrency check (`expected_hash`).
pub fn cuts_set(
    working_root: &Path,
    slug: &str,
    cuts: &CutsJson,
    expected_hash: Option<&str>,
    actor: LockActor,
) -> Result<CutsDocument, ActionError> {
    let ws = safe_workspace_dir(working_root, slug)?;
    let _lock = try_acquire_workspace_lock(
        working_root,
        slug,
        LockLease::new(actor, "cuts:set", "cuts"),
    )?;

    cuts.validate()?;

    let scenes_path = ws.join("detect").join("scenes.json");
    if scenes_path.is_file() {
        let raw_scenes = fs::read_to_string(&scenes_path)?;
        let scenes_data: ScenesJson = serde_json::from_str(&raw_scenes)?;
        let detected_ids: BTreeSet<usize> = scenes_data.scenes.iter().map(|s| s.id).collect();
        validate_ids(cuts, &detected_ids).map_err(ActionError::Validation)?;
    }

    let cuts_path = ws.join("decisions").join("cuts.json");
    if let Some(exp) = expected_hash {
        let actual_hex = if cuts_path.is_file() {
            bytes_sha256_hex(&fs::read(&cuts_path)?)
        } else {
            String::new()
        };
        let expected_hex = normalize_hash_hex(exp);
        if actual_hex != expected_hex {
            return Err(ActionError::HashMismatch {
                expected: expected_hex,
                actual: actual_hex,
            });
        }
    }

    fs::create_dir_all(ws.join("decisions"))?;
    atomic_write_json(&cuts_path, cuts)?;
    let new_bytes = fs::read(&cuts_path)?;
    let new_hash = bytes_sha256_hex(&new_bytes);

    Ok(CutsDocument {
        slug: slug.to_string(),
        hash: new_hash,
        cuts: cuts.clone(),
    })
}

pub fn logs_read(
    working_root: &Path,
    slug: &str,
    stage: &str,
    tail: Option<usize>,
) -> Result<String, ActionError> {
    if StageKind::parse(stage).is_none() {
        return Err(ActionError::UnknownStage(stage.to_string()));
    }
    let ws = safe_workspace_dir(working_root, slug)?;
    let log_path = ws.join("logs").join(format!("{stage}.log"));
    if !log_path.is_file() {
        return Err(ActionError::LogNotFound {
            slug: slug.to_string(),
            stage: stage.to_string(),
        });
    }
    let content = fs::read_to_string(&log_path)?;
    match tail {
        Some(n) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n);
            let mut joined = lines[start..].join("\n");
            if !joined.is_empty() {
                joined.push('\n');
            }
            Ok(joined)
        }
        None => Ok(content),
    }
}
