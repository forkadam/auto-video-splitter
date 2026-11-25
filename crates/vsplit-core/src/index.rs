use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::atomic::atomic_write_json;
use crate::paths::{normalize_rel_path, working_state_dir};
use crate::schemas::index::Index;
use crate::schemas::SchemaError;

pub const INDEX_FILENAME: &str = "index.json";

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid index JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid index schema: {0}")]
    Schema(#[from] SchemaError),
}

pub fn index_path(working_root: &Path) -> PathBuf {
    working_state_dir(working_root).join(INDEX_FILENAME)
}

fn normalize_index_paths(idx: &mut Index) {
    for entry in idx.workspaces.values_mut() {
        entry.source_relative_path = normalize_rel_path(&entry.source_relative_path);
    }
}

pub fn load_index(working_root: &Path) -> Result<Index, IndexError> {
    let p = index_path(working_root);
    if !p.exists() {
        return Ok(Index::default());
    }
    let raw = fs::read_to_string(&p).map_err(|e| IndexError::Io {
        path: p.clone(),
        source: e,
    })?;
    let mut idx: Index =
        serde_json::from_str(&raw).map_err(|e| IndexError::Json { path: p, source: e })?;
    idx.validate()?;
    normalize_index_paths(&mut idx);
    Ok(idx)
}

pub fn save_index(working_root: &Path, idx: &mut Index) -> Result<(), IndexError> {
    idx.validate()?;
    normalize_index_paths(idx);
    let p = index_path(working_root);
    atomic_write_json(&p, idx).map_err(|e| IndexError::Io { path: p, source: e })
}
