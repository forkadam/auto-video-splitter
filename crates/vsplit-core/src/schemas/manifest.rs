use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::SchemaError;

pub const STAGE_NAMES: [&str; 7] = [
    "probe", "detect", "preview", "review", "plan", "split", "verify",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
    AwaitingUser,
    Skipped,
}

impl StageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::AwaitingUser => "awaiting_user",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "awaiting_user" => Some(Self::AwaitingUser),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInfo {
    pub relative_path: String,
    pub size_bytes: u64,
    pub mtime: DateTime<Utc>,
    pub partial_hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataBlock {
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub keyframe_count: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolVersions {
    #[serde(default)]
    pub vsplit: Option<String>,
    #[serde(default)]
    pub ffmpeg: Option<String>,
    #[serde(default)]
    pub ffprobe: Option<String>,
    #[serde(default)]
    pub scenedetect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageError {
    pub message: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub log_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageBlock {
    #[serde(default)]
    pub status: StageStatus,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub set_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, Value>,
    #[serde(default)]
    pub error: Option<StageError>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source: SourceInfo,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub ignored_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: MetadataBlock,
    #[serde(default)]
    pub tool_versions: ToolVersions,
    pub stages: BTreeMap<String, StageBlock>,
    #[serde(default)]
    pub config_snapshot: BTreeMap<String, Value>,
}

impl Manifest {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != 1 {
            return Err(SchemaError::UnsupportedVersion {
                file: "manifest.json",
                found: self.schema_version,
            });
        }
        let expected: BTreeSet<&str> = STAGE_NAMES.iter().copied().collect();
        let actual: BTreeSet<&str> = self.stages.keys().map(String::as_str).collect();
        let missing: Vec<&str> = expected.difference(&actual).copied().collect();
        if !missing.is_empty() {
            return Err(SchemaError::Validation(format!(
                "manifest missing stage keys: {missing:?}"
            )));
        }
        let extra: Vec<&str> = actual.difference(&expected).copied().collect();
        if !extra.is_empty() {
            return Err(SchemaError::Validation(format!(
                "manifest has unknown stage keys: {extra:?}"
            )));
        }
        Ok(())
    }

    pub fn stage(&self, name: &str) -> &StageBlock {
        &self.stages[name]
    }

    pub fn stage_mut(&mut self, name: &str) -> &mut StageBlock {
        self.stages
            .get_mut(name)
            .expect("validated manifest contains all 7 stages")
    }
}
