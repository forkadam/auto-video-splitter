use std::path::PathBuf;

use clap::Args;
use serde_json::json;
use vsplit_core::actions::approve_workspaces;
use vsplit_core::config::load_resolved_config;
use vsplit_core::jobs::JobCoordinator;
use vsplit_core::locks::LockActor;
use vsplit_core::orchestrator::{dispatch, resolve_stages_to_run};
use vsplit_core::scan::adopt_or_find;
use vsplit_core::toolchain::cached_toolchain;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct OneArgs {
    /// Video file path to adopt and process
    pub file_path: PathBuf,
    #[arg(long)]
    pub through: Option<String>,
    #[arg(long)]
    pub auto_approve: bool,
}

pub async fn execute(global: &GlobalOpts, args: OneArgs) -> Result<i32, CliError> {
    let file_path = args
        .file_path
        .canonicalize()
        .map_err(|e| CliError::usage("INVALID_PATH", format!("file not found: {e}")))?;
    if !file_path.is_file() {
        return Err(CliError::usage(
            "INVALID_PATH",
            format!("not a file: {}", file_path.display()),
        ));
    }
    let parent = file_path
        .parent()
        .ok_or_else(|| CliError::usage("INVALID_PATH", "file has no parent directory"))?
        .to_path_buf();

    let slug = adopt_or_find(&parent, &file_path)
        .map_err(|e| CliError::runtime("ADOPT_FAILED", e.to_string()))?;

    let config = load_resolved_config(&parent, global.config_path.as_deref())
        .map_err(|e| CliError::usage("CONFIG_ERROR", e.to_string()))?;
    let toolchain = cached_toolchain();
    let coordinator = JobCoordinator::new();

    let initial_through = args.through.as_deref().unwrap_or("preview");
    let stages = resolve_stages_to_run(None, None, Some(initial_through))
        .map_err(|e| CliError::usage("INVALID_STAGE", e))?;
    let mut summary = dispatch(
        &parent,
        std::slice::from_ref(&slug),
        &config,
        &toolchain,
        &stages,
        &coordinator,
        LockActor::Cli,
    )
    .await;

    if args.auto_approve && !summary.any_failed && !summary.any_locked && !summary.any_missing_tool
    {
        let rev_stages = resolve_stages_to_run(None, None, Some("review")).unwrap();
        let _ = dispatch(
            &parent,
            std::slice::from_ref(&slug),
            &config,
            &toolchain,
            &rev_stages,
            &coordinator,
            LockActor::Cli,
        )
        .await;
        let _ = approve_workspaces(&parent, std::slice::from_ref(&slug), LockActor::Cli);
        let all_stages = resolve_stages_to_run(None, None, None).unwrap();
        summary = dispatch(
            &parent,
            std::slice::from_ref(&slug),
            &config,
            &toolchain,
            &all_stages,
            &coordinator,
            LockActor::Cli,
        )
        .await;
    }

    if summary.any_missing_tool {
        return Err(CliError::new(
            exit_codes::MISSING_DEPENDENCY,
            "MISSING_DEPENDENCY",
            "required external tool (ffmpeg/ffprobe) missing",
        )
        .with_details(serde_json::to_value(&summary).unwrap_or(json!({}))));
    }
    if summary.any_locked {
        return Err(CliError::new(
            exit_codes::WORKSPACE_LOCKED,
            "WORKSPACE_LOCKED",
            format!("workspace {slug:?} is locked by another process"),
        )
        .with_details(serde_json::to_value(&summary).unwrap_or(json!({}))));
    }
    if summary.any_failed {
        return Err(CliError::new(
            exit_codes::RUNTIME_ERROR,
            "STAGE_FAILED",
            format!("processing failed for {slug:?}"),
        )
        .with_details(serde_json::to_value(&summary).unwrap_or(json!({}))));
    }

    emit_ok(
        global.json,
        &json!({
            "slug": slug,
            "working_root": parent.display().to_string(),
            "summary": summary,
        }),
        || {
            println!("target: {} -> {slug}", file_path.display());
        },
    );
    Ok(if summary.any_awaiting_user {
        exit_codes::AWAITING_USER
    } else {
        exit_codes::OK
    })
}
