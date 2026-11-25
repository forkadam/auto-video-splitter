use clap::Args;
use serde_json::json;
use vsplit_core::actions::approve_workspaces;
use vsplit_core::locks::LockActor;

use crate::commands::{resolve_filtered_slugs, GlobalOpts};
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct ApproveArgs {
    pub slug_glob: Option<String>,
    #[arg(long = "filter")]
    pub filters: Vec<String>,
}

pub fn execute(global: &GlobalOpts, args: ApproveArgs) -> Result<i32, CliError> {
    let slugs = resolve_filtered_slugs(global, args.slug_glob.as_deref(), &args.filters)?;
    let approved = approve_workspaces(&global.working_dir, &slugs, LockActor::Cli)
        .map_err(CliError::from_action)?;
    emit_ok(
        global.json,
        &json!({
            "approved": approved.len(),
            "slugs": approved,
        }),
        || {
            for s in &approved {
                println!("approved: {s}");
            }
            println!("\n{} approved", approved.len());
        },
    );
    Ok(exit_codes::OK)
}
