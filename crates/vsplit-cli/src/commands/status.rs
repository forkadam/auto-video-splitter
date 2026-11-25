use std::collections::BTreeMap;

use clap::Args;
use serde::Serialize;
use serde_json::json;
use vsplit_core::index::load_index;
use vsplit_core::locks::{inspect_workspace_lock, LockLease};
use vsplit_core::manifest::load_manifest;
use vsplit_core::paths::workspace_dir;
use vsplit_core::schemas::manifest::{StageStatus, STAGE_NAMES};

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Optional workspace slug
    pub slug: Option<String>,
    #[arg(long)]
    pub awaiting_user: bool,
    #[arg(long)]
    pub failed: bool,
    /// Emit full manifest.json payloads instead of the compact summary projection
    #[arg(long)]
    pub full: bool,
}

#[derive(Debug, Serialize)]
struct CompactWorkspaceStatus {
    slug: String,
    stages: BTreeMap<String, String>,
    scenes_found: Option<u64>,
    ignored: bool,
    locked_by: Option<LockLease>,
    error_summary: Option<String>,
}

pub fn execute(global: &GlobalOpts, args: StatusArgs) -> Result<i32, CliError> {
    let idx = load_index(&global.working_dir)
        .map_err(|e| CliError::runtime("INDEX_ERROR", e.to_string()))?;

    let mut manifests = BTreeMap::new();
    let mut locks = BTreeMap::new();

    for slug in idx.workspaces.keys() {
        if let Some(ref want) = args.slug {
            if slug != want {
                continue;
            }
        }
        let ws = workspace_dir(&global.working_dir, slug);
        let Ok(m) = load_manifest(&ws) else {
            continue;
        };
        if args.awaiting_user
            && !m
                .stages
                .values()
                .any(|b| b.status == StageStatus::AwaitingUser)
        {
            continue;
        }
        if args.failed && !m.stages.values().any(|b| b.status == StageStatus::Failed) {
            continue;
        }
        if let Some(lease) = inspect_workspace_lock(&global.working_dir, slug) {
            locks.insert(slug.clone(), lease);
        }
        manifests.insert(slug.clone(), m);
    }

    if args.full {
        let mut full_map = BTreeMap::new();
        for (slug, m) in &manifests {
            full_map.insert(
                slug.clone(),
                json!({
                    "manifest": m,
                    "locked_by": locks.get(slug),
                }),
            );
        }
        emit_ok(global.json, &json!({ "workspaces": full_map }), || {
            print_table(&manifests, &locks);
        });
    } else {
        let compact: Vec<CompactWorkspaceStatus> = manifests
            .iter()
            .map(|(slug, m)| {
                let stages = STAGE_NAMES
                    .iter()
                    .map(|&s| (s.to_string(), m.stage(s).status.as_str().to_string()))
                    .collect();
                let scenes_found = m
                    .stage("detect")
                    .artifacts
                    .get("scenes_found")
                    .and_then(|v| v.as_u64());
                let error_summary = STAGE_NAMES.iter().find_map(|&s| {
                    let b = m.stage(s);
                    if b.status == StageStatus::Failed {
                        b.error.as_ref().map(|e| format!("{s}: {}", e.message))
                    } else {
                        None
                    }
                });
                CompactWorkspaceStatus {
                    slug: slug.clone(),
                    stages,
                    scenes_found,
                    ignored: m.ignored,
                    locked_by: locks.get(slug).cloned(),
                    error_summary,
                }
            })
            .collect();
        emit_ok(global.json, &json!({ "workspaces": compact }), || {
            print_table(&manifests, &locks);
        });
    }

    Ok(exit_codes::OK)
}

fn print_table(
    manifests: &BTreeMap<String, vsplit_core::schemas::manifest::Manifest>,
    locks: &BTreeMap<String, LockLease>,
) {
    if manifests.is_empty() {
        println!("no workspaces");
        return;
    }
    println!(
        "{:<28} {:<8} {:<8} {:<8} {:<13} {:<8} {:<8} {:<8} FLAGS",
        "SLUG", "PROBE", "DETECT", "PREVIEW", "REVIEW", "PLAN", "SPLIT", "VERIFY"
    );
    for (slug, m) in manifests {
        let mut flags = Vec::new();
        if m.ignored {
            flags.push("ignored".to_string());
        }
        if let Some(l) = locks.get(slug) {
            flags.push(format!("locked({}:{})", l.actor.as_str(), l.stage));
        }
        println!(
            "{:<28} {:<8} {:<8} {:<8} {:<13} {:<8} {:<8} {:<8} {}",
            slug,
            m.stage("probe").status.as_str(),
            m.stage("detect").status.as_str(),
            m.stage("preview").status.as_str(),
            m.stage("review").status.as_str(),
            m.stage("plan").status.as_str(),
            m.stage("split").status.as_str(),
            m.stage("verify").status.as_str(),
            flags.join(",")
        );
    }
}
