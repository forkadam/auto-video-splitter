use std::fs;
use std::io::{self, Read};

use clap::{Args, Subcommand};
use vsplit_core::actions::{cuts_get, cuts_set};
use vsplit_core::locks::LockActor;
use vsplit_core::schemas::cuts::CutsJson;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct CutsArgs {
    #[command(subcommand)]
    pub action: CutsAction,
}

#[derive(Debug, Subcommand)]
pub enum CutsAction {
    /// Read decisions/cuts.json along with its SHA-256 hash
    Get { slug: String },
    /// Validate and atomically write decisions/cuts.json from a file or stdin (`-`)
    Set {
        slug: String,
        /// JSON input path, or `-` to read from stdin
        #[arg(long)]
        file: String,
        /// Optional expected SHA-256 hex digest of existing cuts.json for optimistic locking
        #[arg(long)]
        expected_hash: Option<String>,
    },
}

pub fn execute(global: &GlobalOpts, args: CutsArgs) -> Result<i32, CliError> {
    match args.action {
        CutsAction::Get { slug } => {
            let doc = cuts_get(&global.working_dir, &slug).map_err(CliError::from_action)?;
            emit_ok(global.json, &doc, || {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&doc.cuts).unwrap_or_default()
                );
            });
            Ok(exit_codes::OK)
        }
        CutsAction::Set {
            slug,
            file,
            expected_hash,
        } => {
            let raw = if file == "-" {
                let mut buf = String::new();
                io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| CliError::runtime("STDIN_ERROR", e.to_string()))?;
                buf
            } else {
                fs::read_to_string(&file).map_err(|e| {
                    CliError::usage("INVALID_PATH", format!("cannot read {file}: {e}"))
                })?
            };
            let parsed: CutsJson = serde_json::from_str(&raw)
                .map_err(|e| CliError::usage("INVALID_JSON", format!("invalid cuts.json: {e}")))?;

            let doc = cuts_set(
                &global.working_dir,
                &slug,
                &parsed,
                expected_hash.as_deref(),
                LockActor::Cli,
            )
            .map_err(CliError::from_action)?;

            emit_ok(global.json, &doc, || {
                println!("updated cuts.json for {slug} (hash={})", doc.hash);
            });
            Ok(exit_codes::OK)
        }
    }
}
