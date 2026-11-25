use std::path::PathBuf;

use clap::{Args, ValueEnum};
use vsplit_core::collect::collect_outputs;
use vsplit_core::config::load_resolved_config;
use vsplit_core::filter::{apply_filters, compile_slug_glob};
use vsplit_core::index::load_index;
use vsplit_core::schemas::config::CollectMode;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliCollectMode {
    Copy,
    Move,
    Link,
}

impl From<CliCollectMode> for CollectMode {
    fn from(m: CliCollectMode) -> Self {
        match m {
            CliCollectMode::Copy => Self::Copy,
            CliCollectMode::Move => Self::Move,
            CliCollectMode::Link => Self::Link,
        }
    }
}

#[derive(Debug, Args)]
pub struct CollectArgs {
    /// Optional workspace slugs or glob patterns (defaults to all non-ignored split=done workspaces)
    pub slugs: Vec<String>,
    /// Destination directory (alias: --to)
    #[arg(long = "dest", visible_alias = "to")]
    pub dest: Option<PathBuf>,
    #[arg(long, value_enum)]
    pub mode: Option<CliCollectMode>,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub dry_run: bool,
}

pub fn execute(global: &GlobalOpts, args: CollectArgs) -> Result<i32, CliError> {
    let cfg = load_resolved_config(&global.working_dir, global.config_path.as_deref())
        .map_err(|e| CliError::usage("CONFIG_ERROR", e.to_string()))?;

    let dest_path = match args.dest {
        Some(d) => d,
        None => {
            let configured = PathBuf::from(&cfg.collect.destination);
            if configured.is_absolute() {
                configured
            } else {
                global.working_dir.join(configured)
            }
        }
    };
    let mode: CollectMode = args.mode.map(Into::into).unwrap_or(cfg.collect.mode);

    let resolved_slugs: Option<Vec<String>> = if args.slugs.is_empty() {
        None
    } else {
        let idx = load_index(&global.working_dir)
            .map_err(|e| CliError::runtime("INDEX_ERROR", e.to_string()))?;
        let all: Vec<String> = idx.workspaces.keys().cloned().collect();
        let mut matched = Vec::new();
        for pat in &args.slugs {
            let pred = compile_slug_glob(pat)
                .map_err(|e| CliError::usage("INVALID_FILTER", e.to_string()))?;
            for s in apply_filters(&global.working_dir, &all, &[pred]) {
                if !matched.contains(&s) {
                    matched.push(s);
                }
            }
        }
        Some(matched)
    };

    let report = collect_outputs(
        &global.working_dir,
        resolved_slugs.as_deref(),
        Some(&dest_path),
        mode,
        args.force,
        args.dry_run,
    )
    .map_err(|e| CliError::runtime("COLLECT_FAILED", e.to_string()))?;

    emit_ok(global.json, &report, || {
        println!("destination: {}", report.destination);
        println!("mode: {}", report.mode.as_str());
        println!(
            "{} file(s) from {} workspace(s)",
            report.collected, report.from_workspaces
        );
        if report.skipped_not_done > 0 {
            println!(
                "skipped {} workspace(s): split not done",
                report.skipped_not_done
            );
        }
        if report.skipped_no_output > 0 {
            println!(
                "skipped {} workspace(s): no output files",
                report.skipped_no_output
            );
        }
        if report.skipped_existing > 0 {
            println!(
                "skipped {} file(s) already at destination (re-run with --force to overwrite)",
                report.skipped_existing
            );
        }
    });
    Ok(exit_codes::OK)
}
