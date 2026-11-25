use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::load_app_config;
use crate::process::run_probe_sync;
use crate::schemas::app_config::AppConfig;
use crate::schemas::manifest::ToolVersions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    Env,
    AppToml,
    Path,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTool {
    pub path: PathBuf,
    pub source: ToolSource,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncoderCapabilities {
    pub libx264: bool,
    pub aac: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaToolchain {
    pub ffmpeg: Option<ResolvedTool>,
    pub ffprobe: Option<ResolvedTool>,
    pub encoders: EncoderCapabilities,
}

#[derive(Debug, Error)]
pub enum ToolchainError {
    #[error("missing required external dependency: {0} (configure in app.toml, set VSPLIT_FFMPEG_PATH / VSPLIT_FFPROBE_PATH, or install on PATH)")]
    MissingTool(&'static str),
}

fn find_in_path(binary_name: &str) -> Option<PathBuf> {
    find_all_in_path(binary_name).into_iter().next()
}

pub fn find_all_in_path(binary_name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Some(path_var) = env::var_os("PATH") else {
        return found;
    };
    #[cfg(windows)]
    let candidates: Vec<String> = {
        let lower = binary_name.to_ascii_lowercase();
        if lower.ends_with(".exe") || lower.ends_with(".cmd") || lower.ends_with(".bat") {
            vec![binary_name.to_string()]
        } else {
            vec![
                format!("{binary_name}.exe"),
                format!("{binary_name}.cmd"),
                format!("{binary_name}.bat"),
                binary_name.to_string(),
            ]
        }
    };
    #[cfg(not(windows))]
    let candidates: Vec<String> = vec![binary_name.to_string()];

    for dir in env::split_paths(&path_var) {
        for cand in &candidates {
            let p = dir.join(cand);
            if p.is_file() && is_executable(&p) && !found.contains(&p) {
                found.push(p);
            }
        }
    }
    found
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

fn probe_version(bin: &Path) -> Option<String> {
    let (stdout, _, code) = run_probe_sync(bin, &["-version"]).ok()?;
    if code != Some(0) {
        return None;
    }
    let text = String::from_utf8_lossy(&stdout);
    let first_line = text.lines().next()?.trim();
    first_line
        .split_whitespace()
        .nth(2)
        .map(ToString::to_string)
}

fn probe_encoders(ffmpeg_bin: &Path) -> EncoderCapabilities {
    let Ok((stdout, _, Some(0))) = run_probe_sync(ffmpeg_bin, &["-hide_banner", "-encoders"])
    else {
        return EncoderCapabilities::default();
    };
    let text = String::from_utf8_lossy(&stdout);
    let mut caps = EncoderCapabilities::default();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if parts[1] == "libx264" {
                caps.libx264 = true;
            } else if parts[1] == "aac" {
                caps.aac = true;
            }
        }
    }
    caps
}

fn resolve_one(
    env_var: &str,
    app_path: Option<&PathBuf>,
    default_name: &str,
) -> Option<ResolvedTool> {
    if let Ok(val) = env::var(env_var) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.is_file() {
                let version = probe_version(&p);
                return Some(ResolvedTool {
                    path: p,
                    source: ToolSource::Env,
                    version,
                });
            }
        }
    }

    if let Some(p) = app_path {
        if p.is_file() {
            let version = probe_version(p);
            return Some(ResolvedTool {
                path: p.clone(),
                source: ToolSource::AppToml,
                version,
            });
        }
    }

    if let Some(p) = find_in_path(default_name) {
        let version = probe_version(&p);
        return Some(ResolvedTool {
            path: p,
            source: ToolSource::Path,
            version,
        });
    }

    None
}

impl MediaToolchain {
    /// Discover `ffmpeg` and `ffprobe` via deterministic 3-tier order:
    /// 1. `VSPLIT_FFMPEG_PATH` / `VSPLIT_FFPROBE_PATH`
    /// 2. `app.toml` (`ffmpeg_path` / `ffprobe_path`)
    /// 3. System `PATH`
    pub fn discover(app_cfg: &AppConfig) -> Self {
        let ffmpeg = resolve_one("VSPLIT_FFMPEG_PATH", app_cfg.ffmpeg_path.as_ref(), "ffmpeg");
        let ffprobe = resolve_one(
            "VSPLIT_FFPROBE_PATH",
            app_cfg.ffprobe_path.as_ref(),
            "ffprobe",
        );
        let encoders = ffmpeg
            .as_ref()
            .map(|t| probe_encoders(&t.path))
            .unwrap_or_default();
        Self {
            ffmpeg,
            ffprobe,
            encoders,
        }
    }

    pub fn ffmpeg_path(&self) -> Result<&Path, ToolchainError> {
        self.ffmpeg
            .as_ref()
            .map(|t| t.path.as_path())
            .ok_or(ToolchainError::MissingTool("ffmpeg"))
    }

    pub fn ffprobe_path(&self) -> Result<&Path, ToolchainError> {
        self.ffprobe
            .as_ref()
            .map(|t| t.path.as_path())
            .ok_or(ToolchainError::MissingTool("ffprobe"))
    }

    pub fn tool_versions(&self) -> ToolVersions {
        ToolVersions {
            vsplit: Some(env!("CARGO_PKG_VERSION").to_string()),
            ffmpeg: self.ffmpeg.as_ref().and_then(|t| t.version.clone()),
            ffprobe: self.ffprobe.as_ref().and_then(|t| t.version.clone()),
            scenedetect: None,
        }
    }
}

fn cached_slot() -> &'static Mutex<Option<MediaToolchain>> {
    static CACHE: OnceLock<Mutex<Option<MediaToolchain>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Return the session-cached `MediaToolchain`, probing once on first call.
pub fn cached_toolchain() -> MediaToolchain {
    let mut guard = cached_slot().lock().unwrap();
    if let Some(ref tc) = *guard {
        return tc.clone();
    }
    let app_cfg = load_app_config().unwrap_or_default();
    let tc = MediaToolchain::discover(&app_cfg);
    *guard = Some(tc.clone());
    tc
}

/// Invalidate the session cache (call after updating `app.toml` tool paths).
pub fn refresh_toolchain(app_cfg: &AppConfig) -> MediaToolchain {
    let tc = MediaToolchain::discover(app_cfg);
    let mut guard = cached_slot().lock().unwrap();
    *guard = Some(tc.clone());
    tc
}
