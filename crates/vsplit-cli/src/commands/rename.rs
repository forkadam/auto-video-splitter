use std::path::PathBuf;

use clap::Args;
use vsplit_core::config::load_resolved_config;
use vsplit_core::locks::LockActor;
use vsplit_core::rename::{
    rename_videos, RenameError, RenameOptions, RenameReport, DEFAULT_RENAME_FORMAT,
};
use vsplit_core::toolchain::cached_toolchain;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// Directory containing video files to rename (defaults to --working-dir)
    pub path: Option<PathBuf>,
    /// Case-insensitive filename glob filter (e.g. "IMG*" or "*.mov")
    #[arg(long)]
    pub glob: Option<String>,
    #[arg(long, conflicts_with = "no_recursive")]
    pub recursive: bool,
    #[arg(long, conflicts_with = "recursive")]
    pub no_recursive: bool,
    /// Strftime format for the renamed filename stem
    #[arg(long, default_value = DEFAULT_RENAME_FORMAT)]
    pub format: String,
    /// Preview planned renames without modifying files or .vsplit/
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn execute(global: &GlobalOpts, args: RenameArgs) -> Result<i32, CliError> {
    let working_root = match args.path {
        Some(p) => p
            .canonicalize()
            .map_err(|e| CliError::usage("INVALID_PATH", format!("invalid rename path: {e}")))?,
        None => global.working_dir.clone(),
    };
    if !working_root.is_dir() {
        return Err(CliError::usage(
            "INVALID_PATH",
            format!("not a directory: {}", working_root.display()),
        ));
    }

    let config = load_resolved_config(&working_root, global.config_path.as_deref())
        .map_err(|e| CliError::usage("CONFIG_ERROR", e.to_string()))?;

    let toolchain = cached_toolchain();

    let recursive = if args.recursive {
        Some(true)
    } else if args.no_recursive {
        Some(false)
    } else {
        None
    };

    let opts = RenameOptions {
        recursive,
        glob: args.glob,
        format: Some(args.format),
        dry_run: args.dry_run,
    };

    let report = rename_videos(&working_root, &config, &toolchain, &opts, LockActor::Cli)
        .await
        .map_err(map_rename_error)?;

    print_report(global.json, &report)
}

fn map_rename_error(err: RenameError) -> CliError {
    match err {
        RenameError::InvalidFormat { .. }
        | RenameError::InvalidGlob { .. }
        | RenameError::NotADirectory(_) => CliError::usage("INVALID_ARGUMENT", err.to_string()),
        RenameError::Toolchain(e) => CliError::new(
            exit_codes::MISSING_DEPENDENCY,
            "MISSING_DEPENDENCY",
            e.to_string(),
        ),
        other => CliError::runtime("RENAME_FAILED", other.to_string()),
    }
}

fn print_report(json_mode: bool, report: &RenameReport) -> Result<i32, CliError> {
    emit_ok(json_mode, report, || {
        for w in &report.warnings {
            eprintln!("warning: {w}");
        }
        let verb = if report.dry_run {
            "would rename"
        } else {
            "renamed"
        };
        for item in &report.renamed {
            match (&item.old_slug, &item.new_slug) {
                (Some(old_slug), Some(new_slug)) => {
                    println!(
                        "{verb}: {} -> {} [workspace: {} -> {}]",
                        item.old_path, item.new_path, old_slug, new_slug
                    );
                }
                _ => {
                    println!("{verb}: {} -> {}", item.old_path, item.new_path);
                }
            }
        }
        let c = &report.counts;
        let total_skipped =
            c.skipped_already_named + c.skipped_no_date + c.skipped_locked + c.skipped_error;
        println!(
            "{} renamed, {} workspaces synced, {} skipped (already_named={}, no_date={}, locked={}, error={})",
            c.renamed,
            c.workspaces_synced,
            total_skipped,
            c.skipped_already_named,
            c.skipped_no_date,
            c.skipped_locked,
            c.skipped_error,
        );
    });
    Ok(exit_codes::OK)
}
