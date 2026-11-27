mod commands;
mod exit_codes;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::EnvFilter;

use crate::commands::GlobalOpts;
use crate::output::{emit_err, CliError};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn as_filter(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "vsplit",
    version,
    about = "Local scene-aware video splitter and AI-agent CLI"
)]
struct Cli {
    /// Working directory containing clips and .vsplit/
    #[arg(short = 'w', long = "working-dir", default_value = ".", global = true)]
    working_dir: PathBuf,

    /// Explicit pipeline config.yaml override (never overrides app.toml)
    #[arg(long = "config", global = true)]
    config: Option<PathBuf>,

    /// Emit deterministic single-object JSON on stdout
    #[arg(long = "json", global = true)]
    json: bool,

    /// Never prompt on stdin; fail fast with exit code 2 when confirmation is needed
    #[arg(long = "non-interactive", global = true)]
    non_interactive: bool,

    #[arg(long = "log-level", value_enum, default_value_t = LogLevel::Warn, global = true)]
    log_level: LogLevel,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Discover video files and adopt them into workspaces
    Scan(commands::scan::ScanArgs),
    /// Rename video files by embedded creation date and sync adopted workspaces
    Rename(commands::rename::RenameArgs),
    /// Show pipeline status (compact summary by default under --json; use --full for full manifests)
    Status(commands::status::StatusArgs),
    /// Advance workspaces through pipeline stages
    Run(commands::run::RunArgs),
    /// Mark workspaces awaiting review as approved
    Approve(commands::approve::ApproveArgs),
    /// Mark workspaces as ignored (parked)
    Ignore(commands::ignore::IgnoreArgs),
    /// Clear ignored flag on workspaces
    Unignore(commands::unignore::UnignoreArgs),
    /// Reset workspace stages to pending and clear their stage outputs
    Reset(commands::reset::ResetArgs),
    /// Delete workspaces entirely (never touches source videos)
    Clean(commands::clean::CleanArgs),
    /// Collect split outputs from finished workspaces into one destination folder
    Collect(commands::collect::CollectArgs),
    /// Adopt and process a single video file without scanning sibling files
    One(commands::one::OneArgs),
    /// Inspect or update validated decisions/cuts.json for a workspace
    Cuts(commands::cuts::CutsArgs),
    /// Read per-stage execution logs for a workspace
    Logs(commands::logs::LogsArgs),
    /// Verify ffmpeg/ffprobe, encoders, codecs, PATH shadowing, and active workspace locks
    Doctor(commands::doctor::DoctorArgs),
}

fn raw_args_want_json() -> bool {
    std::env::args_os().any(|a| a == "--json")
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(err) => {
            use clap::error::ErrorKind;
            match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    let _ = err.print();
                    return ExitCode::from(exit_codes::OK as u8);
                }
                _ => {
                    if raw_args_want_json() {
                        emit_err(
                            true,
                            &CliError::usage("USAGE_ERROR", err.to_string().trim()),
                        );
                    } else {
                        let _ = err.print();
                    }
                    return ExitCode::from(exit_codes::USAGE_ERROR as u8);
                }
            }
        }
    };

    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::new(cli.log_level.as_filter()))
        .try_init();

    let working_dir = cli
        .working_dir
        .canonicalize()
        .unwrap_or_else(|_| cli.working_dir.clone());
    let global = GlobalOpts {
        working_dir,
        config_path: cli.config,
        json: cli.json,
        non_interactive: cli.non_interactive,
    };

    let result = match cli.command {
        Command::Scan(args) => commands::scan::execute(&global, args),
        Command::Rename(args) => commands::rename::execute(&global, args).await,
        Command::Status(args) => commands::status::execute(&global, args),
        Command::Run(args) => commands::run::execute(&global, args).await,
        Command::Approve(args) => commands::approve::execute(&global, args),
        Command::Ignore(args) => commands::ignore::execute(&global, args),
        Command::Unignore(args) => commands::unignore::execute(&global, args),
        Command::Reset(args) => commands::reset::execute(&global, args),
        Command::Clean(args) => commands::clean::execute(&global, args),
        Command::Collect(args) => commands::collect::execute(&global, args),
        Command::One(args) => commands::one::execute(&global, args).await,
        Command::Cuts(args) => commands::cuts::execute(&global, args),
        Command::Logs(args) => commands::logs::execute(&global, args),
        Command::Doctor(args) => commands::doctor::execute(&global, args),
    };

    match result {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            emit_err(global.json, &err);
            ExitCode::from(err.exit_code as u8)
        }
    }
}
