use clap::Args;
use serde_json::json;
use vsplit_core::actions::reset_workspaces;
use vsplit_core::locks::LockActor;

use crate::commands::{resolve_filtered_slugs, GlobalOpts};
use crate::exit_codes;
use crate::output::{confirm_or_fail_fast, emit_ok, CliError};

#[derive(Debug, Args)]
pub struct ResetArgs {
    pub slug_glob: Option<String>,
    #[arg(long = "from", conflicts_with = "all")]
    pub from_stage: Option<String>,
    #[arg(long = "all", conflicts_with = "from_stage")]
    pub all: bool,
    #[arg(long = "filter")]
    pub filters: Vec<String>,
    #[arg(long)]
    pub force: bool,
}

pub fn execute(global: &GlobalOpts, args: ResetArgs) -> Result<i32, CliError> {
    if args.from_stage.is_none() && !args.all {
        return Err(CliError::usage(
            "MISSING_ARGUMENT",
            "require --from STAGE or --all",
        ));
    }

    let slugs = resolve_filtered_slugs(global, args.slug_glob.as_deref(), &args.filters)?;
    if args.all && !args.force && !slugs.is_empty() {
        let confirmed = confirm_or_fail_fast(
            &format!("reset ALL stages of {} workspace(s)?", slugs.len()),
            global.non_interactive,
            "Re-run with --force to reset all stages non-interactively",
        )?;
        if !confirmed {
            emit_ok(
                global.json,
                &json!({ "reset": 0, "slugs": [], "aborted": true }),
                || println!("aborted"),
            );
            return Ok(exit_codes::OK);
        }
    }

    let from_eff = if args.all {
        "probe"
    } else {
        args.from_stage.as_deref().unwrap()
    };
    let reset = reset_workspaces(&global.working_dir, &slugs, from_eff, LockActor::Cli)
        .map_err(CliError::from_action)?;

    emit_ok(
        global.json,
        &json!({
            "from": from_eff,
            "reset": reset.len(),
            "slugs": reset,
        }),
        || {
            for s in &reset {
                println!("reset: {s} from={from_eff}");
            }
        },
    );
    Ok(exit_codes::OK)
}
