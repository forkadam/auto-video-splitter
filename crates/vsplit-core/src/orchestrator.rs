use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use rand::RngCore;
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::jobs::{JobCoordinator, JobEvent};
use crate::locks::{try_acquire_workspace_lock, LockActor, LockError, LockLease};
use crate::manifest::{load_manifest, save_manifest};
use crate::paths::{resolve_rel_path, to_posix_rel_path, workspace_dir};
use crate::schemas::config::Config;
use crate::schemas::manifest::{Manifest, StageError, StageStatus, STAGE_NAMES};
use crate::stages::{recover_stage_kind, run_stage_kind, StageContext, StageKind, StageLogger};
use crate::toolchain::MediaToolchain;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WorkspaceRunOutcome {
    Done,
    AwaitingUser,
    Failed { stage: String, message: String },
    MissingTool { stage: String, message: String },
    Locked { lease: LockLease },
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub stages_requested: Vec<String>,
    pub workspaces: BTreeMap<String, WorkspaceRunOutcome>,
    pub any_failed: bool,
    pub any_locked: bool,
    pub any_awaiting_user: bool,
    pub any_missing_tool: bool,
    pub cancelled: bool,
}

fn new_run_id() -> String {
    let mut b = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut b);
    let mut s = String::with_capacity(12);
    for byte in b {
        use std::fmt::Write;
        let _ = write!(&mut s, "{byte:02x}");
    }
    s
}

pub fn resolve_stages_to_run(
    only: Option<&str>,
    from: Option<&str>,
    through: Option<&str>,
) -> Result<Vec<StageKind>, String> {
    if let Some(one) = only {
        let kind = StageKind::parse(one).ok_or_else(|| format!("unknown stage: {one:?}"))?;
        return Ok(vec![kind]);
    }
    let start_idx = match from {
        Some(s) => STAGE_NAMES
            .iter()
            .position(|&n| n == s)
            .ok_or_else(|| format!("unknown --from stage: {s:?}"))?,
        None => 0,
    };
    let end_idx = match through {
        Some(s) => STAGE_NAMES
            .iter()
            .position(|&n| n == s)
            .ok_or_else(|| format!("unknown --through stage: {s:?}"))?,
        None => STAGE_NAMES.len() - 1,
    };
    if start_idx > end_idx {
        return Err(format!(
            "--from ({}) cannot come after --through ({})",
            STAGE_NAMES[start_idx], STAGE_NAMES[end_idx]
        ));
    }
    Ok(STAGE_NAMES[start_idx..=end_idx]
        .iter()
        .map(|&n| StageKind::parse(n).unwrap())
        .collect())
}

fn next_runnable(manifest: &Manifest, stages_to_run: &[StageKind]) -> Option<StageKind> {
    for &kind in stages_to_run {
        let name = kind.name();
        let status = manifest.stage(name).status;
        match status {
            StageStatus::Done | StageStatus::Skipped => continue,
            StageStatus::Failed | StageStatus::AwaitingUser | StageStatus::Running => return None,
            StageStatus::Pending => {
                let idx = STAGE_NAMES.iter().position(|&n| n == name).unwrap();
                if STAGE_NAMES[..idx]
                    .iter()
                    .any(|&earlier| manifest.stage(earlier).status != StageStatus::Done)
                {
                    return None;
                }
                return Some(kind);
            }
        }
    }
    None
}

struct DispatchCtx {
    working_root: PathBuf,
    stages_to_run: Vec<StageKind>,
    config: Arc<Config>,
    toolchain: Arc<MediaToolchain>,
    semaphores: Arc<HashMap<&'static str, Arc<Semaphore>>>,
    coordinator: JobCoordinator,
    actor: LockActor,
}

async fn run_one_workspace(
    dctx: Arc<DispatchCtx>,
    slug: String,
    cancel: CancellationToken,
) -> (String, WorkspaceRunOutcome) {
    let working_root = &dctx.working_root;
    let stages_to_run = &dctx.stages_to_run;
    let config = &dctx.config;
    let toolchain = &dctx.toolchain;
    let semaphores = &dctx.semaphores;
    let coordinator = &dctx.coordinator;
    let actor = dctx.actor;
    let run_id = new_run_id();
    let mut lock_guard = match try_acquire_workspace_lock(
        working_root,
        &slug,
        LockLease::new(actor, "init", &run_id),
    ) {
        Ok(g) => g,
        Err(LockError::Held { lease, .. }) => {
            return (slug, WorkspaceRunOutcome::Locked { lease });
        }
        Err(e) => {
            return (
                slug,
                WorkspaceRunOutcome::Failed {
                    stage: "lock".to_string(),
                    message: e.to_string(),
                },
            );
        }
    };

    let ws_dir = workspace_dir(working_root, &slug);
    let mut manifest = match load_manifest(&ws_dir) {
        Ok(m) => m,
        Err(e) => {
            return (
                slug,
                WorkspaceRunOutcome::Failed {
                    stage: "manifest".to_string(),
                    message: e.to_string(),
                },
            );
        }
    };

    let source_path = resolve_rel_path(working_root, &manifest.source.relative_path);

    // Lease-aware crash recovery: only runs AFTER holding `<slug>.lock` exclusively,
    // and only writes `manifest.json` when at least one `running` stage was recovered.
    let mut recovered_any = false;
    for &stage_name in &STAGE_NAMES {
        if manifest.stage(stage_name).status == StageStatus::Running {
            let kind = StageKind::parse(stage_name).unwrap();
            lock_guard.update_stage(format!("recover:{stage_name}"));
            let logger = StageLogger::new(&ws_dir, stage_name, &run_id);
            logger.warn("recovering interrupted running stage under exclusive workspace lock");
            let mut ctx = StageContext {
                workspace_dir: ws_dir.clone(),
                working_root: working_root.clone(),
                source_path: source_path.clone(),
                manifest: &mut manifest,
                config,
                toolchain,
                logger: &logger,
                cancel: cancel.clone(),
            };
            recover_stage_kind(kind, &mut ctx).await;
            recovered_any = true;
        }
    }
    if recovered_any {
        if let Err(e) = save_manifest(&ws_dir, &mut manifest) {
            return (
                slug,
                WorkspaceRunOutcome::Failed {
                    stage: "recover".to_string(),
                    message: e.to_string(),
                },
            );
        }
    }

    // Unlock reviews that were explicitly approved OR whose decisions/cuts.json was edited
    // since entering `awaiting_user` (so `review::run` re-validates and advances to `done`).
    {
        let rev = manifest.stage("review");
        let should_unlock = if rev.status == StageStatus::AwaitingUser {
            let approved = rev
                .artifacts
                .get("approved_at")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let edited = if !approved {
                let recorded = rev.artifacts.get("cuts_file_hash").and_then(|v| v.as_str());
                let cuts_path = ws_dir.join("decisions").join("cuts.json");
                crate::hash::content_hash(&cuts_path)
                    .ok()
                    .map(|curr| recorded != Some(curr.as_str()))
                    .unwrap_or(false)
            } else {
                false
            };
            approved || edited
        } else {
            false
        };
        if should_unlock {
            manifest.stage_mut("review").status = StageStatus::Pending;
            let _ = save_manifest(&ws_dir, &mut manifest);
        }
    }

    let stage_names: Vec<String> = stages_to_run.iter().map(|s| s.name().to_string()).collect();
    coordinator.emit(JobEvent::RunStarted {
        slug: slug.clone(),
        stages: stage_names,
    });

    let mut final_outcome = WorkspaceRunOutcome::Done;

    loop {
        if cancel.is_cancelled() {
            final_outcome = WorkspaceRunOutcome::Cancelled;
            break;
        }
        let Some(kind) = next_runnable(&manifest, stages_to_run) else {
            // Classify resting state for the requested stages.
            if let Some(failed_stage) = stages_to_run
                .iter()
                .find(|k| manifest.stage(k.name()).status == StageStatus::Failed)
            {
                let msg = manifest
                    .stage(failed_stage.name())
                    .error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| "stage failed".to_string());
                final_outcome = WorkspaceRunOutcome::Failed {
                    stage: failed_stage.name().to_string(),
                    message: msg,
                };
            } else if stages_to_run
                .iter()
                .any(|k| manifest.stage(k.name()).status == StageStatus::AwaitingUser)
            {
                final_outcome = WorkspaceRunOutcome::AwaitingUser;
            }
            break;
        };

        let stage_name = kind.name();
        let sem = Arc::clone(&semaphores[stage_name]);
        let permit = tokio::select! {
            p = sem.acquire_owned() => match p {
                Ok(permit) => permit,
                Err(_) => {
                    final_outcome = WorkspaceRunOutcome::Cancelled;
                    break;
                }
            },
            _ = cancel.cancelled() => {
                final_outcome = WorkspaceRunOutcome::Cancelled;
                break;
            }
        };

        lock_guard.update_stage(stage_name);
        manifest.tool_versions = toolchain.tool_versions();
        manifest.config_snapshot = config.snapshot();
        {
            let block = manifest.stage_mut(stage_name);
            block.status = StageStatus::Running;
            block.started_at = Some(Utc::now());
            block.error = None;
        }
        if let Err(e) = save_manifest(&ws_dir, &mut manifest) {
            drop(permit);
            final_outcome = WorkspaceRunOutcome::Failed {
                stage: stage_name.to_string(),
                message: e.to_string(),
            };
            break;
        }

        coordinator.emit(JobEvent::StageStarted {
            slug: slug.clone(),
            stage: stage_name.to_string(),
        });

        let logger = StageLogger::new(&ws_dir, stage_name, &run_id);
        let stage_res = {
            let mut ctx = StageContext {
                workspace_dir: ws_dir.clone(),
                working_root: working_root.clone(),
                source_path: source_path.clone(),
                manifest: &mut manifest,
                config,
                toolchain,
                logger: &logger,
                cancel: cancel.clone(),
            };
            run_stage_kind(kind, &mut ctx).await
        };
        drop(permit);

        match stage_res {
            Ok(res) => {
                let status = res.status;
                {
                    let block = manifest.stage_mut(stage_name);
                    block.status = status;
                    if status == StageStatus::AwaitingUser {
                        block.set_at = Some(Utc::now());
                    } else {
                        block.completed_at = Some(Utc::now());
                    }
                    for (k, v) in res.artifacts {
                        block.artifacts.insert(k, v);
                    }
                }
                let _ = save_manifest(&ws_dir, &mut manifest);
                coordinator.emit(JobEvent::StageFinished {
                    slug: slug.clone(),
                    stage: stage_name.to_string(),
                    status: status.as_str().to_string(),
                });
                if status == StageStatus::AwaitingUser {
                    final_outcome = WorkspaceRunOutcome::AwaitingUser;
                    break;
                }
            }
            Err(err) if err.is_cancelled() => {
                logger.warn("stage cancelled by user");
                let mut ctx = StageContext {
                    workspace_dir: ws_dir.clone(),
                    working_root: working_root.clone(),
                    source_path: source_path.clone(),
                    manifest: &mut manifest,
                    config,
                    toolchain,
                    logger: &logger,
                    cancel: CancellationToken::new(),
                };
                recover_stage_kind(kind, &mut ctx).await;
                let _ = save_manifest(&ws_dir, &mut manifest);
                final_outcome = WorkspaceRunOutcome::Cancelled;
                break;
            }
            Err(err) => {
                let msg = err.to_string();
                let is_missing = err.is_missing_tool();
                logger.error(&format!("stage failed: {msg}"));
                let rel_log = logger
                    .path()
                    .strip_prefix(working_root)
                    .map(to_posix_rel_path)
                    .unwrap_or_else(|_| format!(".vsplit/workspaces/{slug}/logs/{stage_name}.log"));
                {
                    let block = manifest.stage_mut(stage_name);
                    block.status = StageStatus::Failed;
                    block.error = Some(StageError {
                        message: msg.clone(),
                        exit_code: err.exit_code(),
                        log_path: Some(rel_log),
                    });
                }
                let _ = save_manifest(&ws_dir, &mut manifest);
                coordinator.emit(JobEvent::StageFailed {
                    slug: slug.clone(),
                    stage: stage_name.to_string(),
                    message: msg.clone(),
                });
                final_outcome = if is_missing {
                    WorkspaceRunOutcome::MissingTool {
                        stage: stage_name.to_string(),
                        message: msg,
                    }
                } else {
                    WorkspaceRunOutcome::Failed {
                        stage: stage_name.to_string(),
                        message: msg,
                    }
                };
                break;
            }
        }
    }

    let ok = matches!(
        final_outcome,
        WorkspaceRunOutcome::Done | WorkspaceRunOutcome::AwaitingUser
    );
    coordinator.emit(JobEvent::RunFinished {
        slug: slug.clone(),
        ok,
    });
    (slug, final_outcome)
}

/// Dispatch pipeline stages across `slugs` concurrently under per-stage semaphores and
/// exclusive per-workspace OS locks.
pub async fn dispatch(
    working_root: &Path,
    slugs: &[String],
    config: &Config,
    toolchain: &MediaToolchain,
    stages_to_run: &[StageKind],
    coordinator: &JobCoordinator,
    actor: LockActor,
) -> RunSummary {
    let mut sem_map = HashMap::new();
    for &name in &STAGE_NAMES {
        let limit = config.concurrency.get(name).copied().unwrap_or(1).max(1);
        sem_map.insert(name, Arc::new(Semaphore::new(limit)));
    }
    let dctx = Arc::new(DispatchCtx {
        working_root: working_root.to_path_buf(),
        stages_to_run: stages_to_run.to_vec(),
        config: Arc::new(config.clone()),
        toolchain: Arc::new(toolchain.clone()),
        semaphores: Arc::new(sem_map),
        coordinator: coordinator.clone(),
        actor,
    });

    let mut join_set = JoinSet::new();
    for slug in slugs {
        let tok = coordinator.register(slug);
        let slug_owned = slug.clone();
        let dctx_cl = Arc::clone(&dctx);
        let coord_cl = coordinator.clone();
        join_set.spawn(async move {
            let res = run_one_workspace(dctx_cl, slug_owned.clone(), tok).await;
            coord_cl.unregister(&slug_owned);
            res
        });
    }

    let mut outcomes = BTreeMap::new();
    while let Some(joined) = join_set.join_next().await {
        if let Ok((slug, outcome)) = joined {
            outcomes.insert(slug, outcome);
        }
    }

    let any_failed = outcomes
        .values()
        .any(|o| matches!(o, WorkspaceRunOutcome::Failed { .. }));
    let any_locked = outcomes
        .values()
        .any(|o| matches!(o, WorkspaceRunOutcome::Locked { .. }));
    let any_awaiting_user = outcomes
        .values()
        .any(|o| matches!(o, WorkspaceRunOutcome::AwaitingUser));
    let any_missing_tool = outcomes
        .values()
        .any(|o| matches!(o, WorkspaceRunOutcome::MissingTool { .. }));
    let cancelled = outcomes
        .values()
        .any(|o| matches!(o, WorkspaceRunOutcome::Cancelled));

    RunSummary {
        stages_requested: stages_to_run.iter().map(|s| s.name().to_string()).collect(),
        workspaces: outcomes,
        any_failed,
        any_locked,
        any_awaiting_user,
        any_missing_tool,
        cancelled,
    }
}
