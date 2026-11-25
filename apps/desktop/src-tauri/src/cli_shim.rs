use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tauri::AppHandle;

/// Atomically installs a versioned CLI binary into `bin_store/vsplit-<version>`,
/// swaps `symlink_dir/vsplit` -> `bin_store/vsplit-<version>` (symlink on Unix,
/// atomic copy fallback on Windows for unit testing), and prunes older versions.
pub fn sync_cli_shim(
    bundled_cli: &Path,
    version: &str,
    bin_store: &Path,
    symlink_dir: &Path,
) -> io::Result<PathBuf> {
    fs::create_dir_all(bin_store)?;
    fs::create_dir_all(symlink_dir)?;

    let target_name = format!("vsplit-{version}");
    let target = bin_store.join(&target_name);
    if !target.exists() {
        let tmp = bin_store.join(format!(".vsplit-{version}.tmp"));
        fs::copy(bundled_cli, &tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&tmp)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tmp, perms)?;
        }
        fs::rename(&tmp, &target)?;
    }

    let link_path = symlink_dir.join("vsplit");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let tmp_link = symlink_dir.join(format!(".vsplit-{version}.link"));
        let _ = fs::remove_file(&tmp_link);
        symlink(&target, &tmp_link)?;
        fs::rename(&tmp_link, &link_path)?;
    }
    #[cfg(not(unix))]
    {
        let tmp_copy = symlink_dir.join(format!(".vsplit-{version}.link"));
        let _ = fs::remove_file(&tmp_copy);
        fs::copy(&target, &tmp_copy)?;
        fs::rename(&tmp_copy, &link_path)?;
    }

    // Prune older `vsplit-*` binaries in `bin_store`.
    if let Ok(entries) = fs::read_dir(bin_store) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.starts_with("vsplit-") && s != target_name {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    Ok(link_path)
}

/// Sync the extracted `~/.local/bin/vsplit` CLI shim on Linux AppImage startup or repair.
pub fn sync_if_needed(app: &AppHandle) -> io::Result<String> {
    #[cfg(target_os = "linux")]
    {
        use tauri::Manager;
        let Some(base) = directories::BaseDirs::new() else {
            return Ok("skipped: no home directory resolved".to_string());
        };
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| io::Error::other(e.to_string()))?;

        let exe_sibling = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("vsplit")));

        let mut candidates = Vec::new();
        if let Some(sib) = exe_sibling {
            candidates.push(sib);
        }
        candidates.push(resource_dir.join("vsplit"));
        candidates.push(resource_dir.join("binaries").join("vsplit"));
        candidates.push(
            resource_dir
                .join("binaries")
                .join("vsplit-x86_64-unknown-linux-gnu"),
        );
        let Some(bundled) = candidates.into_iter().find(|p| p.is_file()) else {
            return Ok("skipped: no bundled CLI binary in resource_dir (dev mode)".to_string());
        };

        let bin_store = base.data_local_dir().join("vsplit").join("bin");
        let symlink_dir = base.home_dir().join(".local").join("bin");
        let version = env!("CARGO_PKG_VERSION");
        let linked = sync_cli_shim(&bundled, version, &bin_store, &symlink_dir)?;
        Ok(format!(
            "CLI shim v{version} linked at {}",
            linked.display()
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Ok("Windows installer manages vsplit.exe directly on PATH".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn cli_shim_extracts_atomically_swaps_and_prunes_old_versions() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let bundled_v1 = root.join("src-v1");
        let bundled_v2 = root.join("src-v2");
        fs::write(&bundled_v1, b"binary-v1").unwrap();
        fs::write(&bundled_v2, b"binary-v2").unwrap();

        let bin_store = root.join("share/vsplit/bin");
        let symlink_dir = root.join("local/bin");

        let link1 = sync_cli_shim(&bundled_v1, "0.4.0", &bin_store, &symlink_dir).unwrap();
        assert!(link1.exists());
        assert!(bin_store.join("vsplit-0.4.0").is_file());
        assert_eq!(fs::read(&link1).unwrap(), b"binary-v1");

        // Upgrade to 0.4.1 -> atomically swaps symlink and prunes 0.4.0
        let link2 = sync_cli_shim(&bundled_v2, "0.4.1", &bin_store, &symlink_dir).unwrap();
        assert_eq!(link1, link2);
        assert!(bin_store.join("vsplit-0.4.1").is_file());
        assert!(
            !bin_store.join("vsplit-0.4.0").exists(),
            "old versioned binary should be pruned"
        );
        assert_eq!(fs::read(&link2).unwrap(), b"binary-v2");
    }
}
