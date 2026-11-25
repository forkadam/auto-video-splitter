use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::index::{load_index, IndexError};
use crate::manifest::load_manifest;
use crate::paths::workspace_dir;
use crate::schemas::config::CollectMode;
use crate::schemas::manifest::StageStatus;

#[derive(Debug, Clone, Serialize)]
pub struct CollectReport {
    pub destination: String,
    pub mode: CollectMode,
    pub collected: usize,
    pub from_workspaces: usize,
    pub skipped_not_done: usize,
    pub skipped_no_output: usize,
    pub skipped_existing: usize,
    pub skipped_ignored: usize,
    pub files: Vec<String>,
}

#[derive(Debug, Error)]
pub enum CollectError {
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error("I/O error placing collected file: {0}")]
    Io(#[from] io::Error),
}

fn place_file(src: &Path, dst: &Path, mode: CollectMode) -> io::Result<()> {
    match mode {
        CollectMode::Copy => {
            fs::copy(src, dst)?;
            Ok(())
        }
        CollectMode::Move => match fs::rename(src, dst) {
            Ok(()) => Ok(()),
            Err(_) => {
                fs::copy(src, dst)?;
                fs::remove_file(src)?;
                Ok(())
            }
        },
        CollectMode::Link => match fs::hard_link(src, dst) {
            Ok(()) => Ok(()),
            Err(_) => {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(src, dst)
                }
                #[cfg(windows)]
                {
                    std::os::windows::fs::symlink_file(src, dst)
                }
            }
        },
    }
}

pub fn collect_outputs(
    working_root: &Path,
    slugs: Option<&[String]>,
    dest: Option<&Path>,
    mode: CollectMode,
    force: bool,
    dry_run: bool,
) -> Result<CollectReport, CollectError> {
    let dest_path: PathBuf = match dest {
        Some(d) => d.to_path_buf(),
        None => working_root.join("collected"),
    };
    if !dry_run {
        fs::create_dir_all(&dest_path)?;
    }

    let collect_all = slugs.is_none();
    let idx = load_index(working_root)?;
    let candidates: Vec<String> = match slugs {
        Some(list) => list.to_vec(),
        None => idx.workspaces.keys().cloned().collect(),
    };

    let mut collected = 0usize;
    let mut from_workspaces = 0usize;
    let mut skipped_not_done = 0usize;
    let mut skipped_no_output = 0usize;
    let mut skipped_existing = 0usize;
    let mut skipped_ignored = 0usize;
    let mut files = Vec::new();

    for slug in candidates {
        if !idx.workspaces.contains_key(&slug) {
            continue;
        }
        let ws = workspace_dir(working_root, &slug);
        let Ok(m) = load_manifest(&ws) else {
            continue;
        };

        if collect_all && m.ignored {
            skipped_ignored += 1;
            continue;
        }

        if m.stage("split").status != StageStatus::Done {
            skipped_not_done += 1;
            continue;
        }

        let out_dir = ws.join("output");
        if !out_dir.is_dir() {
            skipped_no_output += 1;
            continue;
        }

        let mut clips: Vec<PathBuf> = fs::read_dir(&out_dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        clips.sort();

        if clips.is_empty() {
            skipped_no_output += 1;
            continue;
        }

        let mut any_placed = false;
        for src in clips {
            let Some(name) = src.file_name() else {
                continue;
            };
            let target = dest_path.join(name);
            if target.exists() && !force {
                skipped_existing += 1;
                continue;
            }
            if !dry_run {
                if target.exists() && force {
                    let _ = fs::remove_file(&target);
                }
                place_file(&src, &target, mode)?;
            }
            files.push(target.display().to_string());
            collected += 1;
            any_placed = true;
        }

        if any_placed {
            from_workspaces += 1;
        }
    }

    Ok(CollectReport {
        destination: dest_path.display().to_string(),
        mode,
        collected,
        from_workspaces,
        skipped_not_done,
        skipped_no_output,
        skipped_existing,
        skipped_ignored,
        files,
    })
}
