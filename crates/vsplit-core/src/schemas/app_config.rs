use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_RECENT_FOLDERS: usize = 10;

fn default_max_recent_folders() -> usize {
    MAX_RECENT_FOLDERS
}

/// Strip Windows extended-length verbatim path prefixes (`\\?\D:\...` -> `D:\...`)
/// produced by `std::fs::canonicalize` so persisted paths stay clean and readable.
pub fn clean_display_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = path.to_string_lossy();
        if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// Desktop-only machine-local settings stored outside `.vsplit/` in `app.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub recent_folders: Vec<PathBuf>,
    pub last_folder: Option<PathBuf>,
    #[serde(default = "default_max_recent_folders")]
    pub max_recent_folders: usize,
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
    pub video_player: Option<PathBuf>,
    pub wayland_disable_dmabuf: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recent_folders: Vec::new(),
            last_folder: None,
            max_recent_folders: default_max_recent_folders(),
            ffmpeg_path: None,
            ffprobe_path: None,
            video_player: None,
            wayland_disable_dmabuf: false,
        }
    }
}

impl AppConfig {
    pub fn effective_max_recent(&self) -> usize {
        self.max_recent_folders.clamp(1, 50)
    }

    pub fn record_recent_folder(&mut self, folder: PathBuf) {
        let cleaned = clean_display_path(folder);
        let cap = self.effective_max_recent();
        self.recent_folders.retain(|p| {
            let norm = clean_display_path(p.clone());
            !paths_equal(&norm, &cleaned)
        });
        self.recent_folders.insert(0, cleaned.clone());
        self.recent_folders.truncate(cap);
        self.last_folder = Some(cleaned);
    }

    /// Normalize Windows verbatim prefixes, deduplicate, and drop folders that no longer exist.
    /// Returns `true` if `recent_folders` or `last_folder` was modified.
    pub fn prune_missing_recent_folders(&mut self) -> bool {
        let orig_recent = self.recent_folders.clone();
        let orig_last = self.last_folder.clone();
        let cap = self.effective_max_recent();

        let mut cleaned: Vec<PathBuf> = Vec::with_capacity(self.recent_folders.len());
        for p in self.recent_folders.drain(..) {
            let norm = clean_display_path(p);
            if norm.is_dir() && !cleaned.iter().any(|existing| paths_equal(existing, &norm)) {
                cleaned.push(norm);
            }
        }
        cleaned.truncate(cap);
        self.recent_folders = cleaned;

        self.last_folder = self
            .last_folder
            .take()
            .map(clean_display_path)
            .filter(|p| p.is_dir());

        self.recent_folders != orig_recent || self.last_folder != orig_last
    }

    pub fn remove_recent_folder(&mut self, target: &Path) -> bool {
        let norm_target = clean_display_path(target.to_path_buf());
        let before = self.recent_folders.len();
        self.recent_folders.retain(|p| {
            let norm = clean_display_path(p.clone());
            !paths_equal(&norm, &norm_target)
        });
        let removed_last = self
            .last_folder
            .as_ref()
            .map(|p| paths_equal(&clean_display_path(p.clone()), &norm_target))
            .unwrap_or(false);
        if removed_last {
            self.last_folder = self.recent_folders.first().cloned();
        }
        self.recent_folders.len() != before || removed_last
    }

    pub fn clear_recent_folders(&mut self) {
        self.recent_folders.clear();
        self.last_folder = None;
    }
}
