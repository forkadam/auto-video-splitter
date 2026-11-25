use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::SchemaError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceEntry {
    pub source_relative_path: String,
    pub size_bytes: u64,
    pub partial_hash: String,
    pub adopted_at: DateTime<Utc>,
}

fn default_schema_version() -> u32 {
    1
}

fn default_working_root() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_working_root")]
    pub working_root: String,
    #[serde(default)]
    pub workspaces: BTreeMap<String, WorkspaceEntry>,
    #[serde(default)]
    pub retired_slugs: Vec<String>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            schema_version: 1,
            working_root: ".".to_string(),
            workspaces: BTreeMap::new(),
            retired_slugs: Vec::new(),
        }
    }
}

impl Index {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != 1 {
            return Err(SchemaError::UnsupportedVersion {
                file: "index.json",
                found: self.schema_version,
            });
        }
        Ok(())
    }
}
