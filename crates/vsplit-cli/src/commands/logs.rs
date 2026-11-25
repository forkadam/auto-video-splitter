use clap::Args;
use serde_json::json;
use vsplit_core::actions::logs_read;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct LogsArgs {
    pub slug: String,
    pub stage: String,
    /// Return only the last N lines
    #[arg(long)]
    pub tail: Option<usize>,
}

pub fn execute(global: &GlobalOpts, args: LogsArgs) -> Result<i32, CliError> {
    let content = logs_read(&global.working_dir, &args.slug, &args.stage, args.tail)
        .map_err(CliError::from_action)?;
    emit_ok(
        global.json,
        &json!({
            "slug": args.slug,
            "stage": args.stage,
            "content": content,
        }),
        || {
            print!("{content}");
        },
    );
    Ok(exit_codes::OK)
}
