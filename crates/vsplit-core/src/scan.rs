use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::partial_hash;
use crate::index::{load_index, save_index, IndexError};
use crate::locks::{lock_index, LockError};
use crate::manifest::{load_manifest, new_manifest, save_manifest, ManifestError};
use crate::paths::{
    project_path_length_estimate, to_posix_rel_path, working_state_dir, workspace_dir, VSPLIT_DIR,
};
use crate::schemas::config::Config;
use crate::schemas::index::WorkspaceEntry;
use crate::schemas::manifest::SourceInfo;
use crate::slugify::{resolve_collision, slugify, SlugError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RelocatePolicy {
    Auto,
    #[default]
    Skip,
    Report,
}

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub recursive: Option<bool>,
    pub relocate_policy: RelocatePolicy,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdoptedItem {
    pub slug: String,
    pub relative_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelocatedItem {
    pub slug: String,
    pub old_path: String,
    pub new_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub adopted: Vec<AdoptedItem>,
    pub skipped: usize,
    pub relocated: Vec<RelocatedItem>,
    pub pending_relocations: Vec<RelocatedItem>,
    pub warnings: Vec<String>,
    pub total_workspaces: usize,
    pub dry_run: bool,
}

#[derive(Debug, Error)]
pub enum ScanError {
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Slug(#[from] SlugError),
    #[error("file {0} is outside working root {1}")]
    OutsideRoot(PathBuf, PathBuf),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

fn collect_candidates(
    working_root: &Path,
    dir: &Path,
    recursive: bool,
    extensions: &HashSet<String>,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        if dir == working_root && file_name == VSPLIT_DIR {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if recursive {
                collect_candidates(working_root, &path, true, extensions, out)?;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        if let Some(ext) = ext {
            if extensions.contains(&ext) {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Headless folder scanner shared by CLI `vsplit scan` and `POST /api/scan`.
/// Holds `.vsplit/.locks/index.lock` during mutations and normalizes all stored paths to POSIX `/`.
pub fn scan_folder(
    working_root: &Path,
    config: &Config,
    opts: &ScanOptions,
) -> Result<ScanReport, ScanError> {
    let recursive = opts.recursive.unwrap_or(config.scan.recursive);
    let extensions: HashSet<String> = config
        .scan
        .extensions
        .iter()
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .collect();

    let mut candidates = Vec::new();
    collect_candidates(
        working_root,
        working_root,
        recursive,
        &extensions,
        &mut candidates,
    )?;
    candidates.sort();

    let _index_guard = if !opts.dry_run {
        Some(lock_index(working_root)?)
    } else {
        None
    };

    let mut idx = load_index(working_root)?;
    let mut live_slugs: HashSet<String> = idx.workspaces.keys().cloned().collect();
    let retired_slugs: HashSet<String> = idx.retired_slugs.iter().cloned().collect();
    let by_hash: HashMap<String, String> = idx
        .workspaces
        .iter()
        .map(|(slug, w)| (w.partial_hash.clone(), slug.clone()))
        .collect();
    let by_path: HashMap<String, String> = idx
        .workspaces
        .iter()
        .map(|(slug, w)| (w.source_relative_path.clone(), slug.clone()))
        .collect();

    let mut adopted = Vec::new();
    let mut skipped = 0usize;
    let mut relocated = Vec::new();
    let mut pending_relocations = Vec::new();
    let mut warnings = Vec::new();

    for path in candidates {
        let rel_path = path
            .strip_prefix(working_root)
            .map_err(|_| ScanError::OutsideRoot(path.clone(), working_root.to_path_buf()))?;
        let rel = to_posix_rel_path(rel_path);
        let meta = fs::metadata(&path)?;
        let size_bytes = meta.len();
        let mtime: DateTime<Utc> = meta
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now());
        let ph = partial_hash(&path)?;

        if let Some(existing_slug) = by_hash.get(&ph) {
            let old_path = idx.workspaces[existing_slug].source_relative_path.clone();
            if old_path != rel {
                match opts.relocate_policy {
                    RelocatePolicy::Auto => {
                        if !opts.dry_run {
                            if let Some(entry) = idx.workspaces.get_mut(existing_slug) {
                                entry.source_relative_path = rel.clone();
                            }
                            let ws = workspace_dir(working_root, existing_slug);
                            let mut m = load_manifest(&ws)?;
                            m.source.relative_path = rel.clone();
                            save_manifest(&ws, &mut m)?;
                        }
                        relocated.push(RelocatedItem {
                            slug: existing_slug.clone(),
                            old_path,
                            new_path: rel,
                        });
                    }
                    RelocatePolicy::Report => {
                        pending_relocations.push(RelocatedItem {
                            slug: existing_slug.clone(),
                            old_path,
                            new_path: rel,
                        });
                        skipped += 1;
                    }
                    RelocatePolicy::Skip => {
                        skipped += 1;
                    }
                }
            } else {
                skipped += 1;
            }
            continue;
        }

        if by_path.contains_key(&rel) {
            warnings.push(format!(
                "source {rel} replaced (hash changed); existing workspace state may be stale"
            ));
            skipped += 1;
            continue;
        }

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let base = slugify(&file_name);
        let new_slug = resolve_collision(&base, &live_slugs, &retired_slugs)?;
        live_slugs.insert(new_slug.clone());

        if project_path_length_estimate(working_root, &new_slug) > 240 {
            warnings.push(format!(
                "path length warning — projected path for {new_slug} exceeds 240 chars; consider a shorter working folder or filename"
            ));
        }

        if !opts.dry_run {
            let ws = workspace_dir(working_root, &new_slug);
            fs::create_dir_all(&ws)?;
            let source = SourceInfo {
                relative_path: rel.clone(),
                size_bytes,
                mtime,
                partial_hash: ph.clone(),
            };
            let mut m = new_manifest(&new_slug, source);
            save_manifest(&ws, &mut m)?;
            idx.workspaces.insert(
                new_slug.clone(),
                WorkspaceEntry {
                    source_relative_path: rel.clone(),
                    size_bytes,
                    partial_hash: ph,
                    adopted_at: Utc::now(),
                },
            );
        }

        adopted.push(AdoptedItem {
            slug: new_slug,
            relative_path: rel,
            size_bytes,
        });
    }

    if !opts.dry_run {
        fs::create_dir_all(working_state_dir(working_root))?;
        save_index(working_root, &mut idx)?;
    }

    let total_workspaces = if opts.dry_run {
        idx.workspaces.len() + adopted.len()
    } else {
        idx.workspaces.len()
    };

    Ok(ScanReport {
        adopted,
        skipped,
        relocated,
        pending_relocations,
        warnings,
        total_workspaces,
        dry_run: opts.dry_run,
    })
}

/// Single-file adoption (`vsplit one` & desktop single-file drop): adopts or reuses
/// one clip without scanning sibling files, under `.vsplit/.locks/index.lock`.
pub fn adopt_or_find(working_root: &Path, file_path: &Path) -> Result<String, ScanError> {
    let rel_path = file_path
        .strip_prefix(working_root)
        .map_err(|_| ScanError::OutsideRoot(file_path.to_path_buf(), working_root.to_path_buf()))?;
    let rel = to_posix_rel_path(rel_path);
    let meta = fs::metadata(file_path)?;
    let size_bytes = meta.len();
    let mtime: DateTime<Utc> = meta
        .modified()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());
    let ph = partial_hash(file_path)?;

    let _index_guard = lock_index(working_root)?;
    let mut idx = load_index(working_root)?;

    if let Some((existing_slug, existing_rel)) = idx
        .workspaces
        .iter()
        .find(|(_, w)| w.partial_hash == ph)
        .map(|(s, w)| (s.clone(), w.source_relative_path.clone()))
    {
        if existing_rel != rel {
            if let Some(entry) = idx.workspaces.get_mut(&existing_slug) {
                entry.source_relative_path = rel.clone();
            }
            let ws = workspace_dir(working_root, &existing_slug);
            let mut m = load_manifest(&ws)?;
            m.source.relative_path = rel;
            save_manifest(&ws, &mut m)?;
            save_index(working_root, &mut idx)?;
        }
        return Ok(existing_slug);
    }

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = slugify(&file_name);
    let live: HashSet<String> = idx.workspaces.keys().cloned().collect();
    let retired: HashSet<String> = idx.retired_slugs.iter().cloned().collect();
    let new_slug = resolve_collision(&base, &live, &retired)?;

    let ws = workspace_dir(working_root, &new_slug);
    fs::create_dir_all(&ws)?;
    let source = SourceInfo {
        relative_path: rel.clone(),
        size_bytes,
        mtime,
        partial_hash: ph.clone(),
    };
    let mut m = new_manifest(&new_slug, source);
    save_manifest(&ws, &mut m)?;
    idx.workspaces.insert(
        new_slug.clone(),
        WorkspaceEntry {
            source_relative_path: rel,
            size_bytes,
            partial_hash: ph,
            adopted_at: Utc::now(),
        },
    );
    fs::create_dir_all(working_state_dir(working_root))?;
    save_index(working_root, &mut idx)?;
    Ok(new_slug)
}
