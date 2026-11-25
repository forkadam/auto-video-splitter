use std::path::{Component, Path, PathBuf};

use directories::BaseDirs;
use thiserror::Error;

pub const VSPLIT_DIR: &str = ".vsplit";
pub const WORKSPACES_DIR: &str = "workspaces";
pub const LOCKS_DIR: &str = ".locks";
const OUTPUT_LABEL_MAX: usize = 32;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("invalid workspace slug {0:?}")]
    InvalidSlug(String),
    #[error("path escapes workspace boundary: {0}")]
    Traversal(String),
}

pub fn working_state_dir(working_root: &Path) -> PathBuf {
    working_root.join(VSPLIT_DIR)
}

pub fn workspaces_root(working_root: &Path) -> PathBuf {
    working_state_dir(working_root).join(WORKSPACES_DIR)
}

pub fn locks_dir(working_root: &Path) -> PathBuf {
    working_state_dir(working_root).join(LOCKS_DIR)
}

pub fn validate_slug(slug: &str) -> Result<(), PathError> {
    if slug.is_empty()
        || slug == "."
        || slug == ".."
        || slug.contains('/')
        || slug.contains('\\')
        || slug.contains('\0')
    {
        return Err(PathError::InvalidSlug(slug.to_string()));
    }
    Ok(())
}

pub fn workspace_dir(working_root: &Path, slug: &str) -> PathBuf {
    workspaces_root(working_root).join(slug)
}

pub fn safe_workspace_dir(working_root: &Path, slug: &str) -> Result<PathBuf, PathError> {
    validate_slug(slug)?;
    Ok(workspace_dir(working_root, slug))
}

/// Normalize a persisted relative path to POSIX `/` separators.
pub fn normalize_rel_path(rel: &str) -> String {
    rel.replace('\\', "/")
}

/// Convert a relative `Path` into a POSIX `/` string for cross-OS persistence.
pub fn to_posix_rel_path(rel: &Path) -> String {
    let mut parts = Vec::new();
    for comp in rel.components() {
        if let Component::Normal(os) = comp {
            parts.push(os.to_string_lossy().into_owned());
        }
    }
    parts.join("/")
}

/// Resolve a POSIX (or legacy Windows) relative path against `working_root`.
pub fn resolve_rel_path(working_root: &Path, rel: &str) -> PathBuf {
    let norm = normalize_rel_path(rel);
    let mut out = working_root.to_path_buf();
    for seg in norm.split('/') {
        if !seg.is_empty() && seg != "." {
            out.push(seg);
        }
    }
    out
}

pub fn project_path_length_estimate(working_root: &Path, slug: &str) -> usize {
    let longest_filename = format!("{slug}-highlight-99-{}.mp4", "X".repeat(OUTPUT_LABEL_MAX));
    let p = workspace_dir(working_root, slug)
        .join("output")
        .join(longest_filename);
    p.to_string_lossy().chars().count()
}

/// Unified user config directory:
/// - `VSPLIT_CONFIG_DIR` env override (if set, used for tests/isolation)
/// - Linux: `$XDG_CONFIG_HOME/vsplit` (default `~/.config/vsplit`)
/// - Windows: `%APPDATA%\vsplit`
pub fn user_config_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("VSPLIT_CONFIG_DIR") {
        if !override_dir.trim().is_empty() {
            return PathBuf::from(override_dir);
        }
    }
    if let Some(base) = BaseDirs::new() {
        return base.config_dir().join("vsplit");
    }
    PathBuf::from(".vsplit-config")
}

/// Unified pipeline `config.yaml` write target (`~/.config/vsplit/config.yaml` or `%APPDATA%\vsplit\config.yaml`).
pub fn user_global_config_path() -> PathBuf {
    user_config_dir().join("config.yaml")
}

/// Legacy Windows `platformdirs` config path (`%LOCALAPPDATA%\vsplit\vsplit\config.yaml`),
/// consulted read-only when `%APPDATA%\vsplit\config.yaml` does not exist.
pub fn legacy_windows_config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if std::env::var_os("VSPLIT_CONFIG_DIR").is_some() {
            return None;
        }
        BaseDirs::new().map(|b| {
            b.data_local_dir()
                .join("vsplit")
                .join("vsplit")
                .join("config.yaml")
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Machine-local desktop config path (`app.toml`, stored outside `.vsplit/`).
pub fn app_config_path() -> PathBuf {
    user_config_dir().join("app.toml")
}

/// Application rotating log directory (`%LOCALAPPDATA%\vsplit\logs` or `$XDG_STATE_HOME/vsplit/logs`).
pub fn app_logs_dir() -> PathBuf {
    if let Some(base) = BaseDirs::new() {
        #[cfg(windows)]
        {
            return base.data_local_dir().join("vsplit").join("logs");
        }
        #[cfg(not(windows))]
        {
            if let Some(state) = base.state_dir() {
                return state.join("vsplit").join("logs");
            }
            return base
                .home_dir()
                .join(".local")
                .join("state")
                .join("vsplit")
                .join("logs");
        }
    }
    PathBuf::from("logs")
}
