use clap::Args;
use serde_json::json;
use vsplit_core::actions::clean_workspaces;
use vsplit_core::index::load_index;
use vsplit_core::locks::LockActor;

use crate::commands::{resolve_filtered_slugs, GlobalOpts};
use crate::exit_codes;
use crate::output::{confirm_or_fail_fast, emit_ok, CliError};

#[derive(Debug, Args)]
pub struct CleanArgs {
    pub slug_glob: Option<String>,
    #[arg(long = "all")]
    pub all: bool,
    #[arg(long = "filter")]
    pub filters: Vec<String>,
    #[arg(long)]
    pub force: bool,
}

pub fn execute(global: &GlobalOpts, args: CleanArgs) -> Result<i32, CliError> {
    let targets = if args.all {
        let idx = load_index(&global.working_dir)
            .map_err(|e| CliError::runtime("INDEX_ERROR", e.to_string()))?;
        idx.workspaces.keys().cloned().collect()
    } else {
        resolve_filtered_slugs(global, args.slug_glob.as_deref(), &args.filters)?
    };

    if targets.is_empty() {
        emit_ok(
            global.json,
            &json!({ "cleaned": 0, "removed": [], "kept_in_use": [] }),
            || println!("nothing to clean"),
        );
        return Ok(exit_codes::OK);
    }

    if !args.force {
        let confirmed = confirm_or_fail_fast(
            &format!("delete {} workspace(s)?", targets.len()),
            global.non_interactive,
            "Re-run with --force to delete workspaces non-interactively",
        )?;
        if !confirmed {
            emit_ok(
                global.json,
                &json!({ "cleaned": 0, "removed": [], "kept_in_use": [], "aborted": true }),
                || println!("aborted"),
            );
            return Ok(exit_codes::OK);
        }
    }

    let result = clean_workspaces(&global.working_dir, &targets, LockActor::Cli)
        .map_err(CliError::from_action)?;

    emit_ok(
        global.json,
        &json!({
            "cleaned": result.removed.len(),
            "removed": result.removed,
            "kept_in_use": result.kept_in_use,
        }),
        || {
            for s in &result.removed {
                println!("cleaned: {s}");
            }
            for s in &result.kept_in_use {
                println!("kept (in use — not deleted): {s}");
            }
            println!("\n{} cleaned", result.removed.len());
        },
    );
    Ok(exit_codes::OK)
}
