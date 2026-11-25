use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::locks::{list_active_locks, LockLease};
use crate::schemas::app_config::AppConfig;
use crate::toolchain::{find_all_in_path, EncoderCapabilities, MediaToolchain, ToolSource};

#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub ok: bool,
    pub path: Option<String>,
    pub source: Option<ToolSource>,
    pub version: Option<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GstreamerStatus {
    pub applicable: bool,
    pub ok: bool,
    pub avdec_h264: bool,
    pub fuse2: bool,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliShimStatus {
    pub current_exe: Option<String>,
    pub version: String,
    pub shim_path: Option<String>,
    pub shim_exists: bool,
    pub local_bin_on_path: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathShadowing {
    pub shadowed: bool,
    pub entries: Vec<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub wayland_session: bool,
    pub wayland_disable_dmabuf: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub ffmpeg: ToolStatus,
    pub ffprobe: ToolStatus,
    pub encoders: EncoderCapabilities,
    pub gstreamer: GstreamerStatus,
    pub cli_shim: CliShimStatus,
    pub path_shadowing: PathShadowing,
    pub locks: BTreeMap<String, LockLease>,
    pub platform: PlatformInfo,
}

fn check_gstreamer() -> GstreamerStatus {
    #[cfg(target_os = "linux")]
    {
        use crate::process::run_probe_sync;
        let avdec = find_all_in_path("gst-inspect-1.0")
            .first()
            .and_then(|bin| run_probe_sync(bin, &["avdec_h264"]).ok())
            .map(|(_, _, code)| code == Some(0))
            .unwrap_or(false);
        let fuse2 = Path::new("/usr/lib/libfuse.so.2").exists()
            || Path::new("/lib/x86_64-linux-gnu/libfuse.so.2").exists()
            || !find_all_in_path("fusermount").is_empty();
        let hint = if !avdec || !fuse2 {
            Some("Install Arch packages: sudo pacman -S ffmpeg gst-libav gst-plugins-good gst-plugins-bad fuse2".to_string())
        } else {
            None
        };
        GstreamerStatus {
            applicable: true,
            ok: avdec,
            avdec_h264: avdec,
            fuse2,
            hint,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        GstreamerStatus {
            applicable: false,
            ok: true,
            avdec_h264: false,
            fuse2: false,
            hint: None,
        }
    }
}

fn check_cli_shim() -> CliShimStatus {
    let current_exe = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string());
    let version = env!("CARGO_PKG_VERSION").to_string();

    #[cfg(target_os = "linux")]
    {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
        let local_bin = home.as_ref().map(|h| h.join(".local").join("bin"));
        let shim = local_bin.as_ref().map(|d| d.join("vsplit"));
        let shim_exists = shim.as_ref().map(|p| p.exists()).unwrap_or(false);
        let local_bin_on_path = match (&local_bin, std::env::var_os("PATH")) {
            (Some(lb), Some(pv)) => std::env::split_paths(&pv).any(|p| p == *lb),
            _ => false,
        };
        CliShimStatus {
            current_exe,
            version,
            shim_path: shim.map(|p| p.display().to_string()),
            shim_exists,
            local_bin_on_path,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        CliShimStatus {
            current_exe,
            version,
            shim_path: None,
            shim_exists: true,
            local_bin_on_path: true,
        }
    }
}

fn check_path_shadowing() -> PathShadowing {
    let entries: Vec<PathBuf> = find_all_in_path("vsplit");
    let strings: Vec<String> = entries.iter().map(|p| p.display().to_string()).collect();
    let shadowed = entries.len() > 1
        && entries
            .first()
            .map(|p| {
                let s = p.to_string_lossy().to_ascii_lowercase();
                s.contains("python")
                    || s.contains(".venv")
                    || s.contains("uv\\tools")
                    || s.contains("uv/tools")
            })
            .unwrap_or(false);
    let warning = if shadowed {
        Some(format!(
            "A legacy Python vsplit binary appears first on PATH: {}",
            strings[0]
        ))
    } else {
        None
    };
    PathShadowing {
        shadowed,
        entries: strings,
        warning,
    }
}

pub fn run_doctor(working_root: Option<&Path>, app_cfg: &AppConfig) -> DoctorReport {
    let tc = MediaToolchain::discover(app_cfg);

    let install_hint = if cfg!(windows) {
        "Install via `winget install Gyan.FFmpeg` or set ffmpeg_path / ffprobe_path in app.toml"
    } else {
        "Install via `sudo pacman -S ffmpeg` or set ffmpeg_path / ffprobe_path in app.toml"
    };

    let ffmpeg = match tc.ffmpeg {
        Some(t) => ToolStatus {
            ok: t.version.is_some(),
            path: Some(t.path.display().to_string()),
            source: Some(t.source),
            version: t.version,
            hint: None,
        },
        None => ToolStatus {
            ok: false,
            path: None,
            source: None,
            version: None,
            hint: Some(install_hint.to_string()),
        },
    };

    let ffprobe = match tc.ffprobe {
        Some(t) => ToolStatus {
            ok: t.version.is_some(),
            path: Some(t.path.display().to_string()),
            source: Some(t.source),
            version: t.version,
            hint: None,
        },
        None => ToolStatus {
            ok: false,
            path: None,
            source: None,
            version: None,
            hint: Some(install_hint.to_string()),
        },
    };

    let gstreamer = check_gstreamer();
    let cli_shim = check_cli_shim();
    let path_shadowing = check_path_shadowing();
    let locks: BTreeMap<String, LockLease> = working_root
        .map(|root| list_active_locks(root).into_iter().collect())
        .unwrap_or_default();

    let wayland_session = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let ok = ffmpeg.ok && ffprobe.ok && tc.encoders.libx264 && tc.encoders.aac && gstreamer.ok;

    DoctorReport {
        ok,
        ffmpeg,
        ffprobe,
        encoders: tc.encoders,
        gstreamer,
        cli_shim,
        path_shadowing,
        locks,
        platform: PlatformInfo {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            wayland_session,
            wayland_disable_dmabuf: app_cfg.wayland_disable_dmabuf,
        },
    }
}
