use clap::Args;
use serde_json::json;
use vsplit_core::config::load_resolved_config;
use vsplit_core::jobs::JobCoordinator;
use vsplit_core::locks::LockActor;
use vsplit_core::orchestrator::{dispatch, resolve_stages_to_run, WorkspaceRunOutcome};
use vsplit_core::schemas::config::VALID_CONCURRENCY_STAGES;
use vsplit_core::toolchain::cached_toolchain;

use crate::commands::{resolve_filtered_slugs, GlobalOpts};
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Optional slug glob pattern
    pub slug_glob: Option<String>,
    #[arg(long, conflicts_with_all = ["from", "through"])]
    pub only: Option<String>,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub through: Option<String>,
    #[arg(long = "filter")]
    pub filters: Vec<String>,
    /// Stage concurrency override `stage=N`, e.g. `split=2`
    #[arg(long = "concurrency")]
    pub concurrency_overrides: Vec<String>,
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn execute(global: &GlobalOpts, args: RunArgs) -> Result<i32, CliError> {
    let mut config = load_resolved_config(&global.working_dir, global.config_path.as_deref())
        .map_err(|e| CliError::usage("CONFIG_ERROR", e.to_string()))?;

    for ov in &args.concurrency_overrides {
        let Some((k, v)) = ov.split_once('=') else {
            return Err(CliError::usage(
                "INVALID_CONCURRENCY",
                format!("--concurrency must be stage=N, got {ov:?}"),
            ));
        };
        if !VALID_CONCURRENCY_STAGES.contains(&k) {
            return Err(CliError::usage(
                "INVALID_CONCURRENCY",
                format!("unknown concurrency stage {k:?}"),
            ));
        }
        let n: usize = v.parse().map_err(|_| {
            CliError::usage(
                "INVALID_CONCURRENCY",
                format!("invalid concurrency count {v:?}"),
            )
        })?;
        if n < 1 {
            return Err(CliError::usage(
                "INVALID_CONCURRENCY",
                "concurrency values must be >= 1",
            ));
        }
        config.concurrency.insert(k.to_string(), n);
    }

    let stages_to_run = resolve_stages_to_run(
        args.only.as_deref(),
        args.from.as_deref(),
        args.through.as_deref(),
    )
    .map_err(|e| CliError::usage("INVALID_STAGE", e))?;

    let slugs = resolve_filtered_slugs(global, args.slug_glob.as_deref(), &args.filters)?;

    if args.dry_run {
        let stage_names: Vec<&str> = stages_to_run.iter().map(|s| s.name()).collect();
        emit_ok(
            global.json,
            &json!({
                "dry_run": true,
                "stages": stage_names,
                "workspaces": slugs,
            }),
            || {
                for s in &slugs {
                    println!("would run: {s}");
                }
            },
        );
        return Ok(exit_codes::OK);
    }

    let toolchain = cached_toolchain();
    let coordinator = JobCoordinator::new();
    let coord_sig = coordinator.clone();
    let sig_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            coord_sig.cancel_all();
        }
    });

    let summary = dispatch(
        &global.working_dir,
        &slugs,
        &config,
        &toolchain,
        &stages_to_run,
        &coordinator,
        LockActor::Cli,
    )
    .await;
    sig_task.abort();

    let exit_code = if summary.cancelled {
        exit_codes::INTERRUPTED
    } else if summary.any_missing_tool {
        exit_codes::MISSING_DEPENDENCY
    } else if summary.any_locked {
        exit_codes::WORKSPACE_LOCKED
    } else if summary.any_failed {
        exit_codes::RUNTIME_ERROR
    } else if summary.any_awaiting_user {
        exit_codes::AWAITING_USER
    } else {
        exit_codes::OK
    };

    if exit_code == exit_codes::OK {
        emit_ok(global.json, &summary, || {
            for (slug, outcome) in &summary.workspaces {
                println!("{slug}: {:?}", outcome);
            }
        });
        Ok(exit_codes::OK)
    } else if exit_code == exit_codes::AWAITING_USER {
        // Non-zero exit code 10 with structured payload so agents know to inspect cuts.json.
        let code = "AWAITING_USER";
        let err = CliError::new(
            exit_codes::AWAITING_USER,
            code,
            "one or more workspaces paused at review (awaiting_user)",
        )
        .with_details(serde_json::to_value(&summary).unwrap_or(json!({})));
        if !global.json {
            for (slug, outcome) in &summary.workspaces {
                if matches!(outcome, WorkspaceRunOutcome::AwaitingUser) {
                    println!("awaiting_user: {slug}");
                }
            }
        }
        Err(err)
    } else {
        let (code, message) = if summary.cancelled {
            ("INTERRUPTED", "run interrupted by signal")
        } else if summary.any_missing_tool {
            (
                "MISSING_DEPENDENCY",
                "required external tool (ffmpeg/ffprobe) missing",
            )
        } else if summary.any_locked {
            (
                "WORKSPACE_LOCKED",
                "one or more workspaces are locked by another process",
            )
        } else {
            ("STAGE_FAILED", "one or more stages failed")
        };
        Err(CliError::new(exit_code, code, message)
            .with_details(serde_json::to_value(&summary).unwrap_or(json!({}))))
    }
}
