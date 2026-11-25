pub mod detect;
pub mod plan;
pub mod preview;
pub mod probe;
pub mod review;
pub mod split;
pub mod verify;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::Value;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::atomic::robust_remove_dir_all;
use crate::process::ProcessError;
use crate::schemas::config::Config;
use crate::schemas::manifest::{Manifest, StageStatus};
use crate::toolchain::{MediaToolchain, ToolchainError};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024; // 2 MB rotation threshold

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    Probe,
    Detect,
    Preview,
    Review,
    Plan,
    Split,
    Verify,
}

impl StageKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "probe" => Some(Self::Probe),
            "detect" => Some(Self::Detect),
            "preview" => Some(Self::Preview),
            "review" => Some(Self::Review),
            "plan" => Some(Self::Plan),
            "split" => Some(Self::Split),
            "verify" => Some(Self::Verify),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Probe => "probe",
            Self::Detect => "detect",
            Self::Preview => "preview",
            Self::Review => "review",
            Self::Plan => "plan",
            Self::Split => "split",
            Self::Verify => "verify",
        }
    }

    pub fn output_subdir(self) -> Option<&'static str> {
        match self {
            Self::Probe => Some("probe"),
            Self::Detect => Some("detect"),
            Self::Preview => Some("preview"),
            Self::Review => Some("decisions"),
            Self::Plan => Some("plan"),
            Self::Split => Some("output"),
            Self::Verify => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum StageExecError {
    #[error("stage cancelled")]
    Cancelled,
    #[error(transparent)]
    MissingTool(#[from] ToolchainError),
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error("{0}")]
    Message(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl StageExecError {
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Process(p) => p.exit_code(),
            _ => None,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Process(ProcessError::Cancelled { .. })
        )
    }

    pub fn is_missing_tool(&self) -> bool {
        matches!(self, Self::MissingTool(_))
    }
}

/// Per-stage rotating log writer (`logs/<stage>.log`) that never keeps OS file handles open
/// across awaits so Windows `clean`/`reset` never hits sharing violations.
pub struct StageLogger {
    path: PathBuf,
}

impl StageLogger {
    pub fn new(workspace_dir: &Path, stage_name: &str, run_id: &str) -> Self {
        let logs = workspace_dir.join("logs");
        let _ = fs::create_dir_all(&logs);
        let path = logs.join(format!("{stage_name}.log"));
        if let Ok(meta) = fs::metadata(&path) {
            if meta.len() > MAX_LOG_BYTES {
                let backup = logs.join(format!("{stage_name}.log.1"));
                let _ = fs::rename(&path, backup);
            }
        }
        let logger = Self { path };
        logger.line(
            "INFO",
            &format!("=== run_id={run_id} stage={stage_name} ==="),
        );
        logger
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn info(&self, msg: &str) {
        self.line("INFO", msg);
    }

    pub fn warn(&self, msg: &str) {
        self.line("WARN", msg);
    }

    pub fn error(&self, msg: &str) {
        self.line("ERROR", msg);
    }

    fn line(&self, level: &str, msg: &str) {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
            let _ = writeln!(f, "{ts} {level} {msg}");
        }
    }
}

pub struct StageContext<'a> {
    pub workspace_dir: PathBuf,
    pub working_root: PathBuf,
    pub source_path: PathBuf,
    pub manifest: &'a mut Manifest,
    pub config: &'a Config,
    pub toolchain: &'a MediaToolchain,
    pub logger: &'a StageLogger,
    pub cancel: CancellationToken,
}

pub struct StageResult {
    pub status: StageStatus,
    pub artifacts: BTreeMap<String, Value>,
}

impl StageResult {
    pub fn done(artifacts: BTreeMap<String, Value>) -> Self {
        Self {
            status: StageStatus::Done,
            artifacts,
        }
    }

    pub fn awaiting_user(artifacts: BTreeMap<String, Value>) -> Self {
        Self {
            status: StageStatus::AwaitingUser,
            artifacts,
        }
    }
}

pub async fn run_stage_kind(
    kind: StageKind,
    ctx: &mut StageContext<'_>,
) -> Result<StageResult, StageExecError> {
    match kind {
        StageKind::Probe => probe::run(ctx).await,
        StageKind::Detect => detect::run(ctx).await,
        StageKind::Preview => preview::run(ctx).await,
        StageKind::Review => review::run(ctx).await,
        StageKind::Plan => plan::run(ctx).await,
        StageKind::Split => split::run(ctx).await,
        StageKind::Verify => verify::run(ctx).await,
    }
}

pub async fn recover_stage_kind(kind: StageKind, ctx: &mut StageContext<'_>) {
    match kind {
        StageKind::Review => review::recover(ctx).await,
        StageKind::Split => split::recover(ctx).await,
        StageKind::Verify => reset_stage_block(ctx.manifest, kind.name()),
        _ => {
            if let Some(sub) = kind.output_subdir() {
                let _ = robust_remove_dir_all(&ctx.workspace_dir.join(sub));
            }
            reset_stage_block(ctx.manifest, kind.name());
        }
    }
}

pub fn reset_stage_block(manifest: &mut Manifest, stage_name: &str) {
    let block = manifest.stage_mut(stage_name);
    block.status = StageStatus::Pending;
    block.artifacts.clear();
    block.error = None;
    block.started_at = None;
    block.completed_at = None;
    block.set_at = None;
}
