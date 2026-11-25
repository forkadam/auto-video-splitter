use std::path::PathBuf;

use clap::Args;
use serde_json::json;
use vsplit_core::config::load_resolved_config;
use vsplit_core::scan::{scan_folder, RelocatePolicy, ScanOptions};

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{confirm_or_fail_fast, emit_ok, CliError};

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Directory to scan (defaults to --working-dir)
    pub path: Option<PathBuf>,
    #[arg(long, conflicts_with = "no_recursive")]
    pub recursive: bool,
    #[arg(long, conflicts_with = "recursive")]
    pub no_recursive: bool,
    #[arg(long, conflicts_with = "no_relocate")]
    pub auto_relocate: bool,
    #[arg(long, conflicts_with = "auto_relocate")]
    pub no_relocate: bool,
    #[arg(long)]
    pub dry_run: bool,
}

pub fn execute(global: &GlobalOpts, args: ScanArgs) -> Result<i32, CliError> {
    let working_root = match args.path {
        Some(p) => p
            .canonicalize()
            .map_err(|e| CliError::usage("INVALID_PATH", format!("invalid scan path: {e}")))?,
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

    let recursive = if args.recursive {
        Some(true)
    } else if args.no_recursive {
        Some(false)
    } else {
        None
    };

    let policy = if args.auto_relocate {
        RelocatePolicy::Auto
    } else if args.no_relocate {
        RelocatePolicy::Skip
    } else {
        RelocatePolicy::Report
    };

    let report = scan_folder(
        &working_root,
        &config,
        &ScanOptions {
            recursive,
            relocate_policy: policy,
            dry_run: args.dry_run,
        },
    )
    .map_err(|e| CliError::runtime("SCAN_FAILED", e.to_string()))?;

    // If neither --auto-relocate nor --no-relocate was specified and renames were found:
    if !args.auto_relocate && !args.no_relocate && !report.pending_relocations.is_empty() {
        let prompt = format!(
            "Rename detected for {} file(s) ({} -> {}). Update pointers?",
            report.pending_relocations.len(),
            report.pending_relocations[0].old_path,
            report.pending_relocations[0].new_path
        );
        let confirmed = confirm_or_fail_fast(
            &prompt,
            global.non_interactive,
            "Re-run with --auto-relocate to update pointers or --no-relocate to skip",
        )?;
        if confirmed {
            let relocated_report = scan_folder(
                &working_root,
                &config,
                &ScanOptions {
                    recursive,
                    relocate_policy: RelocatePolicy::Auto,
                    dry_run: args.dry_run,
                },
            )
            .map_err(|e| CliError::runtime("SCAN_FAILED", e.to_string()))?;
            return print_report(global.json, &relocated_report);
        }
    }

    print_report(global.json, &report)
}

fn print_report(json_mode: bool, report: &vsplit_core::scan::ScanReport) -> Result<i32, CliError> {
    emit_ok(
        json_mode,
        &json!({
            "adopted": report.adopted,
            "skipped": report.skipped,
            "relocated": report.relocated,
            "pending_relocations": report.pending_relocations,
            "warnings": report.warnings,
            "total_workspaces": report.total_workspaces,
            "dry_run": report.dry_run,
        }),
        || {
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            for item in &report.adopted {
                if report.dry_run {
                    println!("would adopt: {} -> {}", item.relative_path, item.slug);
                } else {
                    println!("adopted: {} -> {}", item.relative_path, item.slug);
                }
            }
            for r in &report.relocated {
                println!("relocated: {} ({} -> {})", r.slug, r.old_path, r.new_path);
            }
            println!(
                "\n{} adopted, {} total",
                report.adopted.len(),
                report.total_workspaces
            );
        },
    );
    Ok(exit_codes::OK)
}
