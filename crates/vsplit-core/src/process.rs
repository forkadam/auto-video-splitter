use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub const MAX_STDERR_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr_tail: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub argv: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("failed to spawn {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("subprocess cancelled: {program}")]
    Cancelled { program: String },
    #[error("{program} failed (exit {exit_code:?}): {stderr_tail}")]
    Failed {
        program: String,
        exit_code: Option<i32>,
        stderr_tail: String,
        argv: Vec<String>,
        duration_ms: u64,
    },
    #[error("subprocess I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl ProcessError {
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Failed { exit_code, .. } => *exit_code,
            _ => None,
        }
    }
}

#[cfg(target_os = "linux")]
const APPIMAGE_PATH_LIST_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_PATH",
    "GST_PLUGIN_PATH_1_0",
    "GI_TYPELIB_PATH",
    "XDG_DATA_DIRS",
];

#[cfg(target_os = "linux")]
const APPIMAGE_SINGLE_PATH_VARS: &[&str] = &[
    "GST_PLUGIN_SCANNER",
    "GST_PLUGIN_SCANNER_1_0",
    "GIO_MODULE_DIR",
    "GSETTINGS_SCHEMA_DIR",
    "GTK_PATH",
    "GTK_EXE_PREFIX",
    "GTK_DATA_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
];

/// Compute environment overrides needed when spawning a host binary from inside a
/// Linux AppImage so host tools (`ffmpeg`, `ffprobe`, `gst-inspect-1.0`) do not
/// dynamically link against bundled AppImage libraries or use AppImage plugin paths.
#[cfg(target_os = "linux")]
fn appimage_env_overrides(
    program: &Path,
    appdir: Option<&Path>,
    get_var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, Option<std::ffi::OsString>)> {
    let Some(appdir) = appdir.filter(|p| !p.as_os_str().is_empty()) else {
        return Vec::new();
    };
    if program.starts_with(appdir) {
        return Vec::new();
    }

    let mut overrides = Vec::new();
    for &key in APPIMAGE_PATH_LIST_VARS {
        let Some(val) = get_var(key) else {
            continue;
        };
        let parts: Vec<std::path::PathBuf> = std::env::split_paths(&val).collect();
        let filtered: Vec<std::path::PathBuf> = parts
            .iter()
            .filter(|p| !p.as_os_str().is_empty() && !p.starts_with(appdir))
            .cloned()
            .collect();
        if filtered.len() != parts.len() {
            if filtered.is_empty() {
                overrides.push((key, None));
            } else if let Ok(joined) = std::env::join_paths(filtered) {
                overrides.push((key, Some(joined)));
            }
        }
    }

    for &key in APPIMAGE_SINGLE_PATH_VARS {
        let Some(val) = get_var(key) else {
            continue;
        };
        if Path::new(&val).starts_with(appdir) {
            overrides.push((key, None));
        }
    }

    overrides
}

#[cfg(target_os = "linux")]
fn sanitize_appimage_env_std(cmd: &mut std::process::Command, program: &Path) {
    let appdir = std::env::var_os("APPDIR").map(std::path::PathBuf::from);
    for (key, val) in appimage_env_overrides(program, appdir.as_deref(), std::env::var_os) {
        match val {
            Some(v) => {
                cmd.env(key, v);
            }
            None => {
                cmd.env_remove(key);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn sanitize_appimage_env_std(_cmd: &mut std::process::Command, _program: &Path) {}

#[cfg(target_os = "linux")]
fn sanitize_appimage_env_tokio(cmd: &mut Command, program: &Path) {
    let appdir = std::env::var_os("APPDIR").map(std::path::PathBuf::from);
    for (key, val) in appimage_env_overrides(program, appdir.as_deref(), std::env::var_os) {
        match val {
            Some(v) => {
                cmd.env(key, v);
            }
            None => {
                cmd.env_remove(key);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn sanitize_appimage_env_tokio(_cmd: &mut Command, _program: &Path) {}

#[cfg(windows)]
pub fn apply_no_window_std(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn apply_no_window_std(_cmd: &mut std::process::Command) {}

#[cfg(windows)]
fn apply_no_window_tokio(cmd: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_no_window_tokio(cmd: &mut Command) {
    // Put child in its own process group so cancel terminates the whole tree.
    cmd.process_group(0);
}

#[cfg(windows)]
struct KillGuard {
    job: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for KillGuard {}

#[cfg(windows)]
impl KillGuard {
    fn attach(child: &tokio::process::Child) -> Option<Self> {
        use std::mem::size_of;
        use std::ptr::null_mut;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        let pid = child.id()?;
        unsafe {
            let job = CreateJobObjectW(null_mut(), null_mut());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                CloseHandle(job);
                return None;
            }
            let proc_handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if proc_handle.is_null() {
                CloseHandle(job);
                return None;
            }
            let assigned = AssignProcessToJobObject(job, proc_handle);
            CloseHandle(proc_handle);
            if assigned == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Self { job })
        }
    }

    fn kill_tree(&mut self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if !self.job.is_null() {
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for KillGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        if !self.job.is_null() {
            unsafe {
                CloseHandle(self.job);
            }
            self.job = std::ptr::null_mut();
        }
    }
}

#[cfg(unix)]
struct KillGuard {
    pgid: i32,
}

#[cfg(unix)]
impl KillGuard {
    fn attach(child: &tokio::process::Child) -> Option<Self> {
        child.id().map(|pid| Self { pgid: pid as i32 })
    }

    fn kill_tree(&mut self) {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;
        let pgid = Pid::from_raw(self.pgid);
        let _ = killpg(pgid, Signal::SIGTERM);
        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = killpg(pgid, Signal::SIGKILL);
    }
}

/// Async subprocess runner with `CREATE_NO_WINDOW`, concurrent `stdout`/`stderr` draining
/// (stderr bounded to 16 KB), and OS process-tree teardown on `CancellationToken`.
pub async fn run_command<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    cancel: Option<&CancellationToken>,
) -> Result<ProcessOutput, ProcessError> {
    let t0 = Instant::now();
    let program_str = program.to_string_lossy().into_owned();
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(program_str.clone());
    for a in args {
        argv.push(a.as_ref().to_string_lossy().into_owned());
    }

    if let Some(tok) = cancel {
        if tok.is_cancelled() {
            return Err(ProcessError::Cancelled {
                program: program_str,
            });
        }
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_no_window_tokio(&mut cmd);
    sanitize_appimage_env_tokio(&mut cmd, program);

    let mut child = cmd.spawn().map_err(|e| ProcessError::Spawn {
        program: program_str.clone(),
        source: e,
    })?;
    let mut kill_guard = KillGuard::attach(&child);

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let stdout_task = tokio::spawn(async move {
        let mut out = Vec::new();
        if let Some(ref mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut out).await;
        }
        out
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stderr_pipe {
            let mut chunk = [0u8; 4096];
            loop {
                match pipe.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.len() > MAX_STDERR_BYTES * 2 {
                            let drain = buf.len() - MAX_STDERR_BYTES;
                            buf.drain(..drain);
                        }
                    }
                }
            }
        }
        if buf.len() > MAX_STDERR_BYTES {
            let drain = buf.len() - MAX_STDERR_BYTES;
            buf.drain(..drain);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let status = match cancel {
        Some(tok) => {
            tokio::select! {
                res = child.wait() => res?,
                _ = tok.cancelled() => {
                    if let Some(ref mut g) = kill_guard {
                        g.kill_tree();
                    }
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return Err(ProcessError::Cancelled { program: program_str });
                }
            }
        }
        None => child.wait().await?,
    };

    let stdout = stdout_task.await.unwrap_or_default();
    let stderr_tail = stderr_task.await.unwrap_or_default();
    let duration_ms = t0.elapsed().as_millis() as u64;
    let exit_code = status.code();

    if !status.success() {
        return Err(ProcessError::Failed {
            program: program_str,
            exit_code,
            stderr_tail: stderr_tail.trim().to_string(),
            argv,
            duration_ms,
        });
    }

    Ok(ProcessOutput {
        stdout,
        stderr_tail,
        exit_code,
        duration_ms,
        argv,
    })
}

/// Blocking helper for small quick probes (`-version`, `-encoders`, `gst-inspect-1.0`).
pub fn run_probe_sync<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
) -> Result<(Vec<u8>, String, Option<i32>), std::io::Error> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_no_window_std(&mut cmd);
    sanitize_appimage_env_std(&mut cmd, program);
    let out = cmd.output()?;
    let stderr_slice = if out.stderr.len() > MAX_STDERR_BYTES {
        &out.stderr[out.stderr.len() - MAX_STDERR_BYTES..]
    } else {
        &out.stderr
    };
    Ok((
        out.stdout,
        String::from_utf8_lossy(stderr_slice).into_owned(),
        out.status.code(),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    #[test]
    fn appimage_env_strips_mount_entries_for_host_binary() {
        let appdir = Path::new("/tmp/.mount_vsplitXYZ");
        let vars: HashMap<&str, OsString> = HashMap::from([
            (
                "LD_LIBRARY_PATH",
                OsString::from("/tmp/.mount_vsplitXYZ/usr/lib/:/tmp/.mount_vsplitXYZ/lib/:"),
            ),
            (
                "GST_PLUGIN_SYSTEM_PATH_1_0",
                OsString::from("/tmp/.mount_vsplitXYZ/usr/lib/gstreamer-1.0:"),
            ),
            (
                "XDG_DATA_DIRS",
                OsString::from("/tmp/.mount_vsplitXYZ/usr/share:/usr/share:/usr/local/share"),
            ),
            (
                "GIO_MODULE_DIR",
                OsString::from("/tmp/.mount_vsplitXYZ//usr/lib/gio/modules"),
            ),
        ]);

        let overrides = appimage_env_overrides(
            Path::new("/usr/bin/gst-inspect-1.0"),
            Some(appdir),
            |k| vars.get(k).cloned(),
        );
        let map: HashMap<&str, Option<OsString>> = overrides.into_iter().collect();

        assert_eq!(map.get("LD_LIBRARY_PATH"), Some(&None));
        assert_eq!(map.get("GST_PLUGIN_SYSTEM_PATH_1_0"), Some(&None));
        assert_eq!(
            map.get("XDG_DATA_DIRS"),
            Some(&Some(OsString::from("/usr/share:/usr/local/share")))
        );
        assert_eq!(map.get("GIO_MODULE_DIR"), Some(&None));
    }

    #[test]
    fn appimage_env_preserves_env_for_internal_binary() {
        let appdir = Path::new("/tmp/.mount_vsplitXYZ");
        let vars: HashMap<&str, OsString> = HashMap::from([(
            "LD_LIBRARY_PATH",
            OsString::from("/tmp/.mount_vsplitXYZ/usr/lib/"),
        )]);

        let overrides = appimage_env_overrides(
            Path::new("/tmp/.mount_vsplitXYZ/usr/bin/gst-inspect-1.0"),
            Some(appdir),
            |k| vars.get(k).cloned(),
        );

        assert!(overrides.is_empty());
    }
}
