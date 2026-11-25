use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use thiserror::Error;

use crate::atomic::atomic_write_json;
use crate::paths::normalize_rel_path;
use crate::schemas::manifest::{
    Manifest, MetadataBlock, SourceInfo, StageBlock, StageStatus, ToolVersions, STAGE_NAMES,
};
use crate::schemas::SchemaError;

pub const MANIFEST_FILENAME: &str = "manifest.json";

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest not found at {0}")]
    NotFound(PathBuf),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid manifest JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid manifest schema: {0}")]
    Schema(#[from] SchemaError),
}

pub fn manifest_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join(MANIFEST_FILENAME)
}

pub fn new_manifest(slug: &str, mut source: SourceInfo) -> Manifest {
    source.relative_path = normalize_rel_path(&source.relative_path);
    let now = Utc::now();
    let stages = STAGE_NAMES
        .iter()
        .map(|&name| {
            (
                name.to_string(),
                StageBlock {
                    status: StageStatus::Pending,
                    ..StageBlock::default()
                },
            )
        })
        .collect();
    Manifest {
        schema_version: 1,
        id: slug.to_string(),
        created_at: now,
        updated_at: now,
        source,
        ignored: false,
        ignored_at: None,
        metadata: MetadataBlock::default(),
        tool_versions: ToolVersions::default(),
        stages,
        config_snapshot: BTreeMap::new(),
    }
}

fn normalize_manifest_paths(m: &mut Manifest) {
    m.source.relative_path = normalize_rel_path(&m.source.relative_path);
    for block in m.stages.values_mut() {
        if let Some(ref mut err) = block.error {
            if let Some(ref mut lp) = err.log_path {
                *lp = normalize_rel_path(lp);
            }
        }
    }
}

pub fn load_manifest(workspace_dir: &Path) -> Result<Manifest, ManifestError> {
    let p = manifest_path(workspace_dir);
    if !p.exists() {
        return Err(ManifestError::NotFound(p));
    }
    let raw = fs::read_to_string(&p).map_err(|e| ManifestError::Io {
        path: p.clone(),
        source: e,
    })?;
    let mut m: Manifest =
        serde_json::from_str(&raw).map_err(|e| ManifestError::Json { path: p, source: e })?;
    m.validate()?;
    normalize_manifest_paths(&mut m);
    Ok(m)
}

pub fn save_manifest(workspace_dir: &Path, manifest: &mut Manifest) -> Result<(), ManifestError> {
    manifest.validate()?;
    normalize_manifest_paths(manifest);
    manifest.updated_at = Utc::now();
    let p = manifest_path(workspace_dir);
    atomic_write_json(&p, manifest).map_err(|e| ManifestError::Io { path: p, source: e })
}
