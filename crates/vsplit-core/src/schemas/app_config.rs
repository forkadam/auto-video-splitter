use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const MAX_RECENT_FOLDERS: usize = 10;

/// Desktop-only machine-local settings stored outside `.vsplit/` in `app.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub recent_folders: Vec<PathBuf>,
    pub last_folder: Option<PathBuf>,
    pub ffmpeg_path: Option<PathBuf>,
    pub ffprobe_path: Option<PathBuf>,
    pub wayland_disable_dmabuf: bool,
}

impl AppConfig {
    pub fn record_recent_folder(&mut self, folder: PathBuf) {
        self.recent_folders.retain(|p| p != &folder);
        self.recent_folders.insert(0, folder.clone());
        self.recent_folders.truncate(MAX_RECENT_FOLDERS);
        self.last_folder = Some(folder);
    }
}
