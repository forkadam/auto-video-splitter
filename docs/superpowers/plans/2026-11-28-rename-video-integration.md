# Creation-Date Source Video Renaming (`vsplit rename`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate creation-date source video renaming (`rename-video`) into `vsplit` across `vsplit-core`, `vsplit-cli`, `vsplit-server`, and the Desktop React UI, keeping existing `.vsplit/` workspaces in sync safely under OS locks.

**Architecture:** Implement a two-phase (`plan_renames` → `apply_rename_plan`) engine in `crates/vsplit-core/src/rename.rs` that discovers configured video files, extracts capture timestamps (`com.apple.quicktime.creationdate` preserving recorded offset, falling back to `creation_time` converted to local time, with a shortcut to cached `probe/ffprobe.json`), resolves deterministic duplicate numbering and strict idempotency, and renames source files + `.vsplit/` workspaces under canonical lock order (`index.lock` then `<slug>.lock`). Expose this engine identically via `vsplit rename` CLI, `POST /api/rename` (with Windows watcher unwatch/re-sync handoff), and a live-preview `RenameDialog` in the Desktop toolbar.

**Tech Stack:** Rust (`vsplit-core`, `vsplit-cli`, `vsplit-server`, `chrono`, `globset`, `tokio`, `axum`, `clap`, `serde`), TypeScript + React 19 + shadcn/ui + Tailwind v4 (`frontend`).

**Spec:** `docs/superpowers/specs/2026-11-28-rename-video-integration-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `crates/vsplit-core/src/rename.rs` | Create | Core rename options, report types, format validator, tag extractor, collision/idempotency planner, async `plan_renames`, `apply_rename_plan`, `rename_videos`, and unit tests |
| `crates/vsplit-core/src/lib.rs` | Modify | Export `pub mod rename;` |
| `crates/vsplit-core/src/watcher.rs` | Modify | Add `WorkspaceWatcher::unwatch_workspaces` for releasing Windows directory handles before workspace renames |
| `crates/vsplit-core/tests/rename_videos.rs` | Create | End-to-end core integration tests with real FFmpeg fixtures (unadopted, adopted sync, locked skip, probe cache, glob, recursive) |
| `crates/vsplit-cli/src/commands/rename.rs` | Create | `vsplit rename` clap args, execution, `--json` & human output |
| `crates/vsplit-cli/src/commands/mod.rs` | Modify | Export `pub mod rename;` |
| `crates/vsplit-cli/src/main.rs` | Modify | Wire `Command::Rename` |
| `crates/vsplit-cli/tests/cli_contract.rs` | Modify | Add CLI contract tests for `vsplit --json rename` (dry-run, apply, invalid format exit code `2`) |
| `crates/vsplit-server/src/session.rs` | Modify | Expose `AppState::unwatch_workspaces` |
| `crates/vsplit-server/src/routes/actions.rs` | Modify | Add `RenameBody` and `rename_videos_route` handler (`POST /api/rename`) |
| `crates/vsplit-server/src/routes/mod.rs` | Modify | Register `.route("/rename", post(actions::rename_videos_route))` |
| `crates/vsplit-server/tests/rename_api.rs` | Create | Server API integration test for `POST /api/rename` dry-run + apply with active watcher |
| `frontend/src/lib/api.ts` | Modify | Add `RenameRequest`, `RenameReport`, and `api.rename(...)` |
| `frontend/src/views/RenameDialog.tsx` | Create | Live dry-run preview & confirmation dialog mirroring `CollectDialog.tsx` |
| `frontend/src/App.tsx` | Modify | Add `Rename…` toolbar button and wire `RenameDialog` with `onBeforeMutateWorkspaces` + selection cleanup |
| `README.md` | Modify | Document `vsplit rename` in CLI Commands table |

---

### Task 1: Core Rename Pure Logic (Types, Format Validation, Tag Parsing, Idempotency & Collisions)

**Files:**
- Create: `crates/vsplit-core/src/rename.rs`
- Modify: `crates/vsplit-core/src/lib.rs`

- [ ] **Step 1: Export `rename` module in `crates/vsplit-core/src/lib.rs`**

Add `pub mod rename;` alphabetically to `crates/vsplit-core/src/lib.rs`:

```rust
pub mod paths;
pub mod process;
pub mod rename;
pub mod scan;
```

- [ ] **Step 2: Write `crates/vsplit-core/src/rename.rs` with types, pure helper stubs, and failing unit tests**

Create `crates/vsplit-core/src/rename.rs` containing the public data model (`DEFAULT_RENAME_FORMAT`, `RenameOptions`, `RenameItem`, `RenameSkipReason`, `RenameSkippedItem`, `RenameCounts`, `RenameReport`, `RenameError`), plus unit tests covering format validation, QuickTime & `creation_time` tag extraction, idempotency matching, and per-directory collision resolution:

```rust
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
use globset::{GlobBuilder, GlobMatcher};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::index::IndexError;
use crate::locks::LockError;
use crate::manifest::ManifestError;
use crate::slugify::SlugError;
use crate::toolchain::ToolchainError;

pub const DEFAULT_RENAME_FORMAT: &str = "%Y-%m-%d %H-%M-%S";

const FORBIDDEN_FILENAME_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

#[derive(Debug, Clone, Default)]
pub struct RenameOptions {
    pub recursive: Option<bool>,
    pub glob: Option<String>,
    pub format: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameItem {
    pub old_path: String,
    pub new_path: String,
    pub source_tag: String,
    pub old_slug: Option<String>,
    pub new_slug: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameSkipReason {
    AlreadyNamed,
    NoCreationDate,
    WorkspaceLocked,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameSkippedItem {
    pub path: String,
    pub reason: RenameSkipReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameCounts {
    pub renamed: usize,
    pub workspaces_synced: usize,
    pub skipped_already_named: usize,
    pub skipped_no_date: usize,
    pub skipped_locked: usize,
    pub skipped_error: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameReport {
    pub dry_run: bool,
    pub format: String,
    pub renamed: Vec<RenameItem>,
    pub skipped: Vec<RenameSkippedItem>,
    pub warnings: Vec<String>,
    pub counts: RenameCounts,
}

#[derive(Debug, Error)]
pub enum RenameError {
    #[error("invalid rename format {format:?}: {reason}")]
    InvalidFormat { format: String, reason: String },
    #[error("invalid filename glob {pattern:?}: {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error(transparent)]
    Toolchain(#[from] ToolchainError),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Slug(#[from] SlugError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedTimestamp {
    pub formatted: String,
    pub source_tag: &'static str,
}

pub fn validate_format(fmt: &str) -> Result<(), RenameError> {
    if fmt.trim().is_empty() {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "format string cannot be empty".to_string(),
        });
    }
    if let Some(bad) = fmt
        .chars()
        .find(|c| FORBIDDEN_FILENAME_CHARS.contains(c) || c.is_control())
    {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: format!("contains forbidden filename character {bad:?}"),
        });
    }
    let sample = DateTime::parse_from_rfc3339("2026-05-01T14:23:05+08:00")
        .expect("valid sample timestamp");
    let mut rendered = String::new();
    if write!(&mut rendered, "{}", sample.format(fmt)).is_err() {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "contains invalid strftime specifier".to_string(),
        });
    }
    if rendered.trim().is_empty() {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "formatted output is empty".to_string(),
        });
    }
    if let Some(bad) = rendered
        .chars()
        .find(|c| FORBIDDEN_FILENAME_CHARS.contains(c) || c.is_control())
    {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: format!("formatted output contains forbidden character {bad:?}"),
        });
    }
    if rendered.ends_with('.') || rendered.ends_with(' ') {
        return Err(RenameError::InvalidFormat {
            format: fmt.to_string(),
            reason: "formatted filename stem cannot end with '.' or space".to_string(),
        });
    }
    Ok(())
}

pub(crate) fn compile_filename_glob(pattern: &str) -> Result<GlobMatcher, RenameError> {
    let glob = GlobBuilder::new(pattern)
        .case_insensitive(true)
        .literal_separator(false)
        .build()
        .map_err(|e| RenameError::InvalidGlob {
            pattern: pattern.to_string(),
            source: e,
        })?;
    Ok(glob.compile_matcher())
}

fn find_tag_ci<'a>(tags: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .and_then(|(_, v)| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_fixed_offset(raw: &str) -> Option<DateTime<FixedOffset>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt);
    }
    for pattern in [
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(raw, pattern) {
            return Some(dt);
        }
    }
    None
}

fn parse_creation_time_to_local(raw: &str) -> Option<DateTime<Local>> {
    if let Some(fixed) = parse_fixed_offset(raw) {
        return Some(fixed.with_timezone(&Local));
    }
    for pattern in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, pattern) {
            if let Some(dt) = Local.from_local_datetime(&naive).earliest() {
                return Some(dt);
            }
        }
    }
    None
}

pub(crate) fn extract_creation_timestamp(
    ffprobe_json: &Value,
    fmt: &str,
) -> Option<ExtractedTimestamp> {
    let format_tags = ffprobe_json
        .get("format")
        .and_then(|f| f.get("tags"))
        .and_then(Value::as_object);

    if let Some(tags) = format_tags {
        if let Some(raw) = find_tag_ci(tags, "com.apple.quicktime.creationdate") {
            if let Some(dt) = parse_fixed_offset(raw) {
                return Some(ExtractedTimestamp {
                    formatted: dt.format(fmt).to_string(),
                    source_tag: "com.apple.quicktime.creationdate",
                });
            }
        }
        if let Some(raw) = find_tag_ci(tags, "creation_time") {
            if let Some(dt) = parse_creation_time_to_local(raw) {
                return Some(ExtractedTimestamp {
                    formatted: dt.format(fmt).to_string(),
                    source_tag: "creation_time",
                });
            }
        }
    }

    if let Some(streams) = ffprobe_json.get("streams").and_then(Value::as_array) {
        let video_stream = streams
            .iter()
            .find(|s| s.get("codec_type").and_then(Value::as_str) == Some("video"));
        if let Some(tags) = video_stream
            .and_then(|s| s.get("tags"))
            .and_then(Value::as_object)
        {
            if let Some(raw) = find_tag_ci(tags, "creation_time") {
                if let Some(dt) = parse_creation_time_to_local(raw) {
                    return Some(ExtractedTimestamp {
                        formatted: dt.format(fmt).to_string(),
                        source_tag: "creation_time",
                    });
                }
            }
        }
    }

    None
}

/// Returns true if `current_stem` already equals `desired_stem` (case-insensitive)
/// or `<desired_stem>(<n>)` where `<n>` is a positive integer (>= 1).
pub(crate) fn is_already_named(current_stem: &str, desired_stem: &str) -> bool {
    if current_stem.eq_ignore_ascii_case(desired_stem) {
        return true;
    }
    let cur_lower = current_stem.to_ascii_lowercase();
    let want_lower = desired_stem.to_ascii_lowercase();
    if let Some(rest) = cur_lower.strip_prefix(&want_lower) {
        if let Some(inner) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            if let Ok(n) = inner.parse::<u64>() {
                return n >= 1;
            }
        }
    }
    false
}

/// Allocate the next available `<desired_stem>.<ext>` or `<desired_stem>(n).<ext>`
/// in `occupied_lower` (mutating `occupied_lower` to reserve the chosen lowercase name).
pub(crate) fn allocate_target_filename(
    desired_stem: &str,
    ext: &str,
    occupied_lower: &mut HashSet<String>,
) -> String {
    let base = format!("{desired_stem}.{ext}");
    if occupied_lower.insert(base.to_ascii_lowercase()) {
        return base;
    }
    let mut n: u64 = 1;
    loop {
        let candidate = format!("{desired_stem}({n}).{ext}");
        if occupied_lower.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_format_accepts_default_and_rejects_illegal_chars() {
        assert!(validate_format(DEFAULT_RENAME_FORMAT).is_ok());
        assert!(validate_format("%Y%m%d_%H%M%S").is_ok());
        assert!(validate_format("").is_err());
        assert!(validate_format("   ").is_err());
        assert!(validate_format("%Y-%m-%d %H:%M:%S").is_err());
        assert!(validate_format("%Y/%m/%d").is_err());
        assert!(validate_format("%Y\\m").is_err());
        assert!(validate_format("%Y-%Q").is_err());
        assert!(validate_format("%Y-%m-%d.").is_err());
        assert!(validate_format("%Y-%m-%d ").is_err());
    }

    #[test]
    fn extract_prefers_quicktime_creationdate_preserving_offset() {
        let meta = json!({
            "format": {
                "tags": {
                    "com.apple.quicktime.creationdate": "2025-06-12T14:30:05+0800",
                    "creation_time": "2025-06-12T06:30:05.000000Z"
                }
            }
        });
        let got = extract_creation_timestamp(&meta, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(got.formatted, "2025-06-12 14-30-05");
        assert_eq!(got.source_tag, "com.apple.quicktime.creationdate");
    }

    #[test]
    fn extract_handles_rfc3339_offset_and_case_insensitive_keys() {
        let meta = json!({
            "format": {
                "tags": {
                    "Com.Apple.QuickTime.CreationDate": "2026-01-03T09:05:11-05:00"
                }
            }
        });
        let got = extract_creation_timestamp(&meta, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(got.formatted, "2026-01-03 09-05-11");
        assert_eq!(got.source_tag, "com.apple.quicktime.creationdate");
    }

    #[test]
    fn extract_falls_back_to_format_and_stream_creation_time() {
        let meta = json!({
            "format": { "tags": { "creation_time": "2025-06-12T06:30:05.000000Z" } }
        });
        let got = extract_creation_timestamp(&meta, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(got.source_tag, "creation_time");
        assert_eq!(got.formatted.len(), "2025-06-12 14-30-05".len());

        let stream_only = json!({
            "format": { "tags": {} },
            "streams": [
                { "codec_type": "audio", "tags": {} },
                { "codec_type": "video", "tags": { "creation_time": "2025-06-12T06:30:05Z" } }
            ]
        });
        let got_stream = extract_creation_timestamp(&stream_only, DEFAULT_RENAME_FORMAT).unwrap();
        assert_eq!(got_stream.source_tag, "creation_time");

        let empty = json!({ "format": { "tags": {} }, "streams": [] });
        assert!(extract_creation_timestamp(&empty, DEFAULT_RENAME_FORMAT).is_none());
    }

    #[test]
    fn idempotency_matches_exact_and_numbered_stems() {
        assert!(is_already_named("2026-05-01 14-23-05", "2026-05-01 14-23-05"));
        assert!(is_already_named("2026-05-01 14-23-05(1)", "2026-05-01 14-23-05"));
        assert!(is_already_named("2026-05-01 14-23-05(12)", "2026-05-01 14-23-05"));
        assert!(!is_already_named("2026-05-01 14-23-05(0)", "2026-05-01 14-23-05"));
        assert!(!is_already_named("2026-05-01 14-23-05-extra", "2026-05-01 14-23-05"));
        assert!(!is_already_named("IMG_1234", "2026-05-01 14-23-05"));
    }

    #[test]
    fn allocate_target_filename_handles_duplicates_and_case_collisions() {
        let mut occupied = HashSet::from(["2026-05-01 14-23-05.mov".to_string()]);
        let first = allocate_target_filename("2026-05-01 14-23-05", "MOV", &mut occupied);
        assert_eq!(first, "2026-05-01 14-23-05(1).MOV");
        let second = allocate_target_filename("2026-05-01 14-23-05", "MOV", &mut occupied);
        assert_eq!(second, "2026-05-01 14-23-05(2).MOV");
        let mp4 = allocate_target_filename("2026-05-01 14-23-05", "mp4", &mut occupied);
        assert_eq!(mp4, "2026-05-01 14-23-05.mp4");
    }
}
```

- [ ] **Step 3: Run unit tests to verify they compile and pass**

Run: `cargo test -p vsplit-core rename::tests`
Expected: 5 tests pass (`validate_format_accepts_default_and_rejects_illegal_chars`, `extract_prefers_quicktime_creationdate_preserving_offset`, `extract_handles_rfc3339_offset_and_case_insensitive_keys`, `extract_falls_back_to_format_and_stream_creation_time`, `idempotency_matches_exact_and_numbered_stems`, `allocate_target_filename_handles_duplicates_and_case_collisions` — 6 tests).

- [ ] **Step 4: Commit**

```powershell
git add crates/vsplit-core/src/lib.rs crates/vsplit-core/src/rename.rs
git commit -m "feat(core): add creation-date rename types, tag parser, and collision planner"
```

---

### Task 2: Core Rename Async Planner, Executor, Workspace Sync & Integration Tests

**Files:**
- Modify: `crates/vsplit-core/src/rename.rs`
- Create: `crates/vsplit-core/tests/rename_videos.rs`

- [ ] **Step 1: Write integration tests in `crates/vsplit-core/tests/rename_videos.rs`**

Create `crates/vsplit-core/tests/rename_videos.rs` testing:
1. Unadopted folder dry-run vs. apply with FFmpeg-generated MOV (`com.apple.quicktime.creationdate` via `-movflags use_metadata_tags`) and MP4 (`creation_time`) fixtures, plus duplicate same-second clips and idempotent second run.
2. Adopted workspace sync: `scan_folder` → `rename_videos` renames both the file and `.vsplit/workspaces/<old-slug>` → `<new-slug>`, preserves `decisions/cuts.json` and `manifest.stages`, and updates `index.json` + `manifest.source.relative_path`.
3. Locked workspace skipped with `RenameSkipReason::WorkspaceLocked` while unlocked sibling renames cleanly.
4. `--glob "IMG*"` and `--recursive` filtering.
5. Cached `probe/ffprobe.json` shortcut works even without re-probing.

```rust
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use vsplit_core::atomic::atomic_write_json;
use vsplit_core::index::load_index;
use vsplit_core::locks::{try_acquire_workspace_lock, LockActor, LockLease};
use vsplit_core::manifest::load_manifest;
use vsplit_core::paths::workspace_dir;
use vsplit_core::rename::{rename_videos, RenameOptions, RenameSkipReason};
use vsplit_core::scan::{scan_folder, ScanOptions};
use vsplit_core::schemas::config::Config;
use vsplit_core::schemas::cuts::{CutsJson, Scene};
use vsplit_core::toolchain::resolve_toolchain;

fn make_clip(
    ffmpeg: &Path,
    dest: &Path,
    quicktime_creation: Option<&str>,
    creation_time: Option<&str>,
) {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-y",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=blue:s=64x64:d=0.5:r=10",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
    ]);
    if let Some(qt) = quicktime_creation {
        cmd.args(["-movflags", "use_metadata_tags"]);
        cmd.args(["-metadata", &format!("com.apple.quicktime.creationdate={qt}")]);
    }
    if let Some(ct) = creation_time {
        cmd.args(["-metadata", &format!("creation_time={ct}")]);
    }
    cmd.arg(dest);
    let status = cmd.status().expect("spawn ffmpeg");
    assert!(status.success(), "ffmpeg failed creating {}", dest.display());
}

#[tokio::test]
async fn rename_unadopted_dry_run_apply_duplicates_and_idempotency() {
    let tc = resolve_toolchain();
    let Ok(ffmpeg) = tc.ffmpeg_path() else {
        eprintln!("skipping test: ffmpeg not available");
        return;
    };
    if tc.ffprobe_path().is_err() {
        eprintln!("skipping test: ffprobe not available");
        return;
    }

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    make_clip(ffmpeg, &root.join("IMG_0001.MOV"), Some("2025-06-12T14:30:05+0800"), None);
    make_clip(ffmpeg, &root.join("IMG_0002.MOV"), Some("2025-06-12T14:30:05+0800"), None);
    make_clip(ffmpeg, &root.join("other.mp4"), None, Some("2025-07-01T10:00:00Z"));
    make_clip(ffmpeg, &root.join("nodate.mp4"), None, None);

    let cfg = Config::default();

    // 1. Dry-run leaves disk untouched
    let dry = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            glob: Some("IMG*".into()),
            dry_run: true,
            ..Default::default()
        },
        LockActor::Cli,
    )
    .await
    .unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.counts.renamed, 2);
    assert_eq!(dry.renamed[0].old_path, "IMG_0001.MOV");
    assert_eq!(dry.renamed[0].new_path, "2025-06-12 14-30-05.MOV");
    assert_eq!(dry.renamed[1].old_path, "IMG_0002.MOV");
    assert_eq!(dry.renamed[1].new_path, "2025-06-12 14-30-05(1).MOV");
    assert!(root.join("IMG_0001.MOV").exists());

    // 2. Apply with glob "IMG*"
    let applied = rename_videos(
        root,
        &cfg,
        &tc,
        &RenameOptions {
            glob: Some("IMG*".into()),
            dry_run: false,
            ..Default::default()
        },
        LockActor::Cli,
    )
    .await
    .unwrap();
    assert!(!applied.dry_run);
    assert_eq!(applied.counts.renamed, 2);
    assert!(root.join("2025-06-12 14-30-05.MOV").exists());
    assert!(root.join("2025-06-12 14-30-05(1).MOV").exists());
    assert!(root.join("other.mp4").exists());

    // 3. Re-running on all files is idempotent for already-renamed files and renames other.mp4
    let second = rename_videos(root, &cfg, &tc, &RenameOptions::default(), LockActor::Cli)
        .await
        .unwrap();
    assert_eq!(second.counts.skipped_already_named, 2);
    assert_eq!(second.counts.renamed, 1);
    assert_eq!(second.counts.skipped_no_date, 1);
    assert_eq!(second.renamed[0].old_path, "other.mp4");
    assert_eq!(second.renamed[0].source_tag, "creation_time");
    assert!(second.renamed[0].new_path.ends_with(".mp4"));
}

#[tokio::test]
async fn rename_syncs_adopted_workspace_slug_and_skips_locked() {
    let tc = resolve_toolchain();
    let Ok(ffmpeg) = tc.ffmpeg_path() else {
        return;
    };
    if tc.ffprobe_path().is_err() {
        return;
    }

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    make_clip(ffmpeg, &root.join("IMG_0100.MOV"), Some("2026-03-10T08:15:20+0800"), None);
    make_clip(ffmpeg, &root.join("IMG_0200.MOV"), Some("2026-03-10T09:00:00+0800"), None);

    let cfg = Config::default();
    let scan = scan_folder(root, &cfg, &ScanOptions::default()).unwrap();
    assert_eq!(scan.adopted.len(), 2);

    // Add decisions/cuts.json to img-0100 so we verify workspace contents are preserved
    let old_ws = workspace_dir(root, "img-0100");
    fs::create_dir_all(old_ws.join("decisions")).unwrap();
    let cuts = CutsJson {
        schema_version: 1,
        scenes: vec![Scene {
            id: 1,
            start: "00:00:00.000".into(),
            end: "00:00:00.400".into(),
            keep: true,
            label: Some("intro".into()),
            merge_with_next: false,
        }],
    };
    atomic_write_json(&old_ws.join("decisions/cuts.json"), &cuts).unwrap();

    // Lock img-0200 to verify it is skipped safely
    let _lock = try_acquire_workspace_lock(
        root,
        "img-0200",
        LockLease::new(LockActor::Cli, "detect", "test-run"),
    )
    .unwrap();

    let report = rename_videos(root, &cfg, &tc, &RenameOptions::default(), LockActor::Desktop)
        .await
        .unwrap();

    assert_eq!(report.counts.renamed, 1);
    assert_eq!(report.counts.workspaces_synced, 1);
    assert_eq!(report.counts.skipped_locked, 1);
    assert_eq!(report.renamed[0].old_slug.as_deref(), Some("img-0100"));
    assert_eq!(
        report.renamed[0].new_slug.as_deref(),
        Some("2026-03-10-08-15-20")
    );
    assert_eq!(report.skipped[0].reason, RenameSkipReason::WorkspaceLocked);

    // Verify source file + workspace directory + manifest + index updated
    assert!(root.join("2026-03-10 08-15-20.MOV").exists());
    assert!(root.join("IMG_0200.MOV").exists());
    let new_ws = workspace_dir(root, "2026-03-10-08-15-20");
    assert!(new_ws.join("decisions/cuts.json").exists());
    assert!(!old_ws.exists());

    let manifest = load_manifest(&new_ws).unwrap();
    assert_eq!(manifest.id, "2026-03-10-08-15-20");
    assert_eq!(manifest.source.relative_path, "2026-03-10 08-15-20.MOV");

    let idx = load_index(root).unwrap();
    assert!(!idx.workspaces.contains_key("img-0100"));
    assert!(idx.workspaces.contains_key("2026-03-10-08-15-20"));
    assert!(idx.workspaces.contains_key("img-0200"));
    assert_eq!(
        idx.workspaces["2026-03-10-08-15-20"].source_relative_path,
        "2026-03-10 08-15-20.MOV"
    );
}
```

- [ ] **Step 2: Run the integration test to confirm it fails (functions not yet defined)**

Run: `cargo test -p vsplit-core --test rename_videos`
Expected: FAIL with `unresolved import vsplit_core::rename::rename_videos`

- [ ] **Step 3: Implement `plan_renames`, `apply_rename_plan`, and `rename_videos` in `crates/vsplit-core/src/rename.rs`**

Add the complete two-phase async planner and atomic executor to `crates/vsplit-core/src/rename.rs`:
- `pub struct RenamePlan` (holds `format: String`, `entries: Vec<PlannedRename>`, `skipped: Vec<RenameSkippedItem>`, `warnings: Vec<String>`, `has_index: bool`, plus helper `pub fn affected_slugs(&self) -> Vec<String>` and `pub fn into_dry_run_report(self) -> RenameReport`).
- `pub async fn plan_renames(working_root: &Path, config: &Config, toolchain: &Toolchain, opts: &RenameOptions) -> Result<RenamePlan, RenameError>`:
  1. Validates `working_root.is_dir()`, `validate_format(&fmt)`, compiles optional `glob`.
  2. Discovers candidate video files matching `config.scan.extensions` (skipping `.vsplit/`), respecting `recursive`, applying optional filename glob, and sorting by POSIX relative path.
  3. Loads `index.json` if `.vsplit/index.json` exists; maps `source_relative_path -> slug` (and falls back to `partial_hash` if needed).
  4. Checks `inspect_workspace_lock` for workspace-backed candidates (recording `WorkspaceLocked` immediately).
  5. Probes remaining candidates concurrently (bounded semaphore = 4):
     - Checks cached `.vsplit/workspaces/<slug>/probe/ffprobe.json` first (when `size_bytes` matches `idx.workspaces[slug].size_bytes`).
     - Otherwise invokes `ffprobe -v error -print_format json -show_format -show_streams <path>` via `crate::process::run_command` (requiring `toolchain.ffprobe_path()?` only when at least one uncached file needs probing — wait: verify `toolchain.ffprobe_path()?` upfront if any uncached candidate exists, returning `RenameError::Toolchain` so missing `ffprobe` fails fast with exit code 4!).
     - Extracts timestamp via `extract_creation_timestamp`.
  6. Per parent directory, classifies `is_already_named` items as `AlreadyNamed`, initializes `occupied_lower` from all `fs::read_dir(parent)` entries minus moving sources, and allocates deterministic target names via `allocate_target_filename`.
  7. Computes projected `new_slug` for workspace-backed items using `slugify(&new_filename)` and `resolve_collision(&base, &live_slugs, &retired_slugs)` (updating `live_slugs` cleanly as planned workspaces move).
- `pub fn apply_rename_plan(working_root: &Path, plan: RenamePlan, actor: LockActor) -> Result<RenameReport, RenameError>`:
  1. If `plan.entries.is_empty()`, returns report immediately without acquiring locks.
  2. If `plan.has_index` (or `.vsplit/index.json` exists), acquires `lock_index(working_root)?` and reloads fresh `index.json`.
  3. Handles cycle / case-only collisions across the batch: any planned item whose `new_abs` currently exists on disk AND equals another planned item's `old_abs` (or case-insensitively equals its own `old_abs`) is first staged to a unique temp filename `.vsplit-rename-tmp-<rand>-<file_name>` in the same directory; then every item moves to its final `new_abs`.
  4. For each workspace-backed item:
     - Acquires `try_acquire_workspace_lock(working_root, &old_slug, LockLease::new(actor, "rename", &run_id))` BEFORE renaming its source file! If `LockError::Held`, skips both source rename and workspace update and records `WorkspaceLocked`.
     - Renames source file (`old_abs` → `new_abs`).
     - Recomputes `new_slug` against live `idx` (handles concurrent changes since planning).
     - If `new_slug != old_slug`: attempts `fs::rename(workspace_dir(working_root, &old_slug), workspace_dir(working_root, &new_slug))`.
       - On `Ok(())`: loads `manifest.json` from `new_ws`, updates `id = new_slug` & `source.relative_path = new_rel`, saves manifest, re-keys `idx.workspaces`.
       - On `Err(e)`: keeps `old_slug`, updates `manifest.source.relative_path = new_rel`, updates `idx.workspaces[&old_slug].source_relative_path = new_rel`, pushes warning `"renamed source file {old_path} -> {new_path}, but could not rename workspace directory {old_slug} ({e}); kept slug {old_slug} with updated source path"`.
     - If `new_slug == old_slug`: updates `source.relative_path` in `manifest.json` and `idx.workspaces`.
  5. Saves `index.json` atomically if modified, builds `RenameCounts` and returns `RenameReport`.
- `pub async fn rename_videos(working_root: &Path, config: &Config, toolchain: &Toolchain, opts: &RenameOptions, actor: LockActor) -> Result<RenameReport, RenameError>`:
  Calls `plan_renames`; if `opts.dry_run`, returns `plan.into_dry_run_report()`; otherwise runs `apply_rename_plan`.

- [ ] **Step 4: Run core unit + integration tests**

Run: `cargo test -p vsplit-core rename`
Expected: All unit tests in `rename::tests` and integration tests in `tests/rename_videos.rs` PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/vsplit-core/src/rename.rs crates/vsplit-core/tests/rename_videos.rs
git commit -m "feat(core): implement async rename planner, atomic executor, and workspace sync"
```

---

### Task 3: Watcher Unwatch Helper for Windows-Safe Workspace Renames

**Files:**
- Modify: `crates/vsplit-core/src/watcher.rs`
- Modify: `crates/vsplit-server/src/session.rs`

- [ ] **Step 1: Add `unwatch_workspaces` to `WorkspaceWatcher` in `crates/vsplit-core/src/watcher.rs`**

Add method to `impl WorkspaceWatcher` right after `sync_watches`:

```rust
    /// Temporarily unwatch specific workspace directories (and their `decisions/`
    /// subdirectories) before renaming or moving them. On Windows, open
    /// `ReadDirectoryChangesW` handles can otherwise block `fs::rename`.
    pub fn unwatch_workspaces(&mut self, working_root: &Path, slugs: &[String]) {
        let Some(ref mut debouncer) = self._native else {
            return;
        };
        for slug in slugs {
            let ws = crate::paths::workspace_dir(working_root, slug);
            let dec = ws.join("decisions");
            let _ = debouncer.watcher().unwatch(&dec);
            let _ = debouncer.watcher().unwatch(&ws);
        }
    }
```

- [ ] **Step 2: Expose `AppState::unwatch_workspaces` in `crates/vsplit-server/src/session.rs`**

Add method to `impl AppState` right after `sync_watches`:

```rust
    pub fn unwatch_workspaces(&self, slugs: &[String]) {
        if slugs.is_empty() {
            return;
        }
        let mut guard = self.project.lock().unwrap();
        let root = guard.working_root.clone();
        if let (Some(ref r), Some(ref mut w)) = (root, guard.watch.as_mut()) {
            w.watcher.unwatch_workspaces(r, slugs);
        }
    }
```

- [ ] **Step 3: Run existing core & server watcher tests**

Run: `cargo test -p vsplit-server --test watcher_sse`
Expected: PASS

- [ ] **Step 4: Commit**

```powershell
git add crates/vsplit-core/src/watcher.rs crates/vsplit-server/src/session.rs
git commit -m "feat(watcher): add unwatch_workspaces helper for Windows-safe workspace renames"
```

---

### Task 4: CLI Subcommand `vsplit rename` & Contract Tests

**Files:**
- Create: `crates/vsplit-cli/src/commands/rename.rs`
- Modify: `crates/vsplit-cli/src/commands/mod.rs`
- Modify: `crates/vsplit-cli/src/main.rs`
- Modify: `crates/vsplit-cli/tests/cli_contract.rs`

- [ ] **Step 1: Write CLI contract test in `crates/vsplit-cli/tests/cli_contract.rs`**

Append a test to `crates/vsplit-cli/tests/cli_contract.rs` verifying:
- `vsplit -w <dir> --json rename --format "%Y:%m"` exits with code `2` (`USAGE_ERROR`) and `"ok": false`.
- `vsplit -w <dir> --json rename --dry-run` on an empty folder exits `0` with `"ok": true`, `"dry_run": true`, `"counts": { "renamed": 0, ... }`.
- `vsplit -w <dir> --json rename --glob "IMG*" --dry-run` and apply on an FFmpeg-created clip with QuickTime metadata renames the clip and reports `"renamed": 1`.

- [ ] **Step 2: Create `crates/vsplit-cli/src/commands/rename.rs`**

```rust
use std::path::PathBuf;

use clap::Args;
use vsplit_core::config::load_resolved_config;
use vsplit_core::locks::LockActor;
use vsplit_core::rename::{rename_videos, RenameError, RenameOptions, DEFAULT_RENAME_FORMAT};
use vsplit_core::toolchain::resolve_toolchain;

use crate::commands::GlobalOpts;
use crate::exit_codes;
use crate::output::{emit_ok, CliError};

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// Directory containing video clips (defaults to --working-dir)
    pub path: Option<PathBuf>,
    /// Case-insensitive filename glob filter (e.g. "IMG*" or "*.mov")
    #[arg(long)]
    pub glob: Option<String>,
    #[arg(long, conflicts_with = "no_recursive")]
    pub recursive: bool,
    #[arg(long, conflicts_with = "recursive")]
    pub no_recursive: bool,
    /// Strftime format for the new filename stem (without extension)
    #[arg(long, default_value = DEFAULT_RENAME_FORMAT)]
    pub format: String,
    /// Preview renames without modifying files or .vsplit/
    #[arg(long)]
    pub dry_run: bool,
}

fn map_rename_error(err: RenameError) -> CliError {
    match err {
        RenameError::InvalidFormat { .. }
        | RenameError::InvalidGlob { .. }
        | RenameError::NotADirectory(_) => CliError::usage("INVALID_ARGUMENT", err.to_string()),
        RenameError::Toolchain(e) => {
            CliError::new(exit_codes::MISSING_DEPENDENCY, "MISSING_DEPENDENCY", e.to_string())
        }
        other => CliError::runtime("RENAME_FAILED", other.to_string()),
    }
}

pub async fn execute(global: &GlobalOpts, args: RenameArgs) -> Result<i32, CliError> {
    let working_root = match args.path {
        Some(p) => p
            .canonicalize()
            .map_err(|e| CliError::usage("INVALID_PATH", format!("invalid rename path: {e}")))?,
        None => global.working_dir.clone(),
    };
    if !working_root.is_dir() {
        return Err(CliError::usage(
            "INVALID_PATH",
            format!("not a directory: {}", working_root.display()),
        ));
    }

    let config = load_resolved_config(&working_root, global.config_path.as_deref())
        .map_err(|e| CliError::usage("CONFIG_ERROR", e.to_string()))?;

    let recursive = if args.recursive {
        Some(true)
    } else if args.no_recursive {
        Some(false)
    } else {
        None
    };

    let toolchain = resolve_toolchain();
    let opts = RenameOptions {
        recursive,
        glob: args.glob,
        format: Some(args.format),
        dry_run: args.dry_run,
    };

    let report = rename_videos(&working_root, &config, &toolchain, &opts, LockActor::Cli)
        .await
        .map_err(map_rename_error)?;

    emit_ok(global.json, &report, || {
        for w in &report.warnings {
            eprintln!("warning: {w}");
        }
        let verb = if report.dry_run { "would rename" } else { "renamed" };
        for item in &report.renamed {
            match (&item.old_slug, &item.new_slug) {
                (Some(old_s), Some(new_s)) if old_s != new_s => {
                    println!(
                        "{verb}: {} -> {} [workspace: {old_s} -> {new_s}]",
                        item.old_path, item.new_path
                    );
                }
                (Some(old_s), _) => {
                    println!(
                        "{verb}: {} -> {} [workspace: {old_s}]",
                        item.old_path, item.new_path
                    );
                }
                _ => {
                    println!("{verb}: {} -> {}", item.old_path, item.new_path);
                }
            }
        }
        let c = &report.counts;
        println!(
            "\n{} {verb} ({} workspace(s) synced), skipped: {} already named, {} no date, {} locked, {} error(s)",
            c.renamed,
            c.workspaces_synced,
            c.skipped_already_named,
            c.skipped_no_date,
            c.skipped_locked,
            c.skipped_error
        );
    });

    Ok(exit_codes::OK)
}
```

- [ ] **Step 3: Wire `rename` in `crates/vsplit-cli/src/commands/mod.rs` and `crates/vsplit-cli/src/main.rs`**

In `crates/vsplit-cli/src/commands/mod.rs`: add `pub mod rename;`.
In `crates/vsplit-cli/src/main.rs`:
- Add `Rename(commands::rename::RenameArgs),` to `enum Command` with doc comment `/// Rename source videos on disk using embedded creation-date metadata`.
- Add `Command::Rename(args) => commands::rename::execute(&global, args).await,` to the match in `main()`.

- [ ] **Step 4: Run CLI tests**

Run: `cargo test -p vsplit-cli`
Expected: All CLI contract tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/vsplit-cli/src/commands/rename.rs crates/vsplit-cli/src/commands/mod.rs crates/vsplit-cli/src/main.rs crates/vsplit-cli/tests/cli_contract.rs
git commit -m "feat(cli): add vsplit rename subcommand with --dry-run, --glob, --format, and --json"
```

---

### Task 5: HTTP API Endpoint `POST /api/rename` & Server Integration Tests

**Files:**
- Modify: `crates/vsplit-server/src/routes/actions.rs`
- Modify: `crates/vsplit-server/src/routes/mod.rs`
- Create: `crates/vsplit-server/tests/rename_api.rs`

- [ ] **Step 1: Add `RenameBody` and `rename_videos_route` to `crates/vsplit-server/src/routes/actions.rs`**

```rust
#[derive(Deserialize, Default)]
pub struct RenameBody {
    pub glob: Option<String>,
    pub recursive: Option<bool>,
    pub format: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

fn map_rename_api_error(err: vsplit_core::rename::RenameError) -> ApiError {
    use vsplit_core::rename::RenameError;
    match err {
        RenameError::InvalidFormat { .. }
        | RenameError::InvalidGlob { .. }
        | RenameError::NotADirectory(_) => ApiError::unprocessable(err.to_string()),
        RenameError::Toolchain(e) => {
            ApiError::new(axum::http::StatusCode::SERVICE_UNAVAILABLE, e.to_string())
        }
        other => ApiError::internal(other.to_string()),
    }
}

pub async fn rename_videos_route(
    State(state): State<AppState>,
    body: Option<Json<RenameBody>>,
) -> Result<Json<Value>, ApiError> {
    let root = require_working_root(&state)?;
    let req = body.map(|b| b.0).unwrap_or_default();
    let cfg =
        load_resolved_config(&root, None).map_err(|e| ApiError::unprocessable(e.to_string()))?;
    let toolchain = cached_toolchain();
    let opts = vsplit_core::rename::RenameOptions {
        recursive: req.recursive,
        glob: req.glob.filter(|g| !g.trim().is_empty()),
        format: req.format.filter(|f| !f.trim().is_empty()),
        dry_run: req.dry_run,
    };

    let plan = vsplit_core::rename::plan_renames(&root, &cfg, &toolchain, &opts)
        .await
        .map_err(map_rename_api_error)?;

    if req.dry_run {
        let report = plan.into_dry_run_report();
        return Ok(Json(serde_json::to_value(report).map_err(|e| ApiError::internal(e.to_string()))?));
    }

    let affected_slugs = plan.affected_slugs();
    state.unwatch_workspaces(&affected_slugs);

    let root_for_apply = root.clone();
    let report = tokio::task::spawn_blocking(move || {
        vsplit_core::rename::apply_rename_plan(&root_for_apply, plan, LockActor::Desktop)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(map_rename_api_error)?;

    state.sync_watches();

    Ok(Json(
        serde_json::to_value(report).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}
```

- [ ] **Step 2: Register `/rename` route in `crates/vsplit-server/src/routes/mod.rs`**

Add `.route("/rename", post(actions::rename_videos_route))` right after `.route("/scan", post(project::scan))`.

- [ ] **Step 3: Create `crates/vsplit-server/tests/rename_api.rs`**

Test `POST /api/rename` against a live `start_server(Some(root))` instance with an adopted FFmpeg clip while the `WorkspaceWatcher` is active:
1. `POST /api/rename` with `{ "format": "%Y:%m", "dry_run": true }` returns `422`.
2. `POST /api/rename` with `{ "dry_run": true }` returns `200`, `dry_run: true`, `counts.renamed == 1`, file untouched.
3. `POST /api/rename` with `{ "dry_run": false }` returns `200`, `counts.renamed == 1`, `counts.workspaces_synced == 1`, `warnings` empty (confirming `unwatch_workspaces` prevents Windows handle errors).
4. `GET /api/workspaces` returns the new date-based slug.

- [ ] **Step 4: Run server tests**

Run: `cargo test -p vsplit-server`
Expected: All server tests including `rename_api` PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/vsplit-server/src/routes/actions.rs crates/vsplit-server/src/routes/mod.rs crates/vsplit-server/tests/rename_api.rs
git commit -m "feat(server): add POST /api/rename endpoint with watcher handoff"
```

---

### Task 6: Desktop UI (`RenameDialog`), `api.ts`, `App.tsx`, `README.md`, and Full Verification

**Files:**
- Modify: `frontend/src/lib/api.ts`
- Create: `frontend/src/views/RenameDialog.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `README.md`

- [ ] **Step 1: Add `RenameRequest`, `RenameReport`, and `api.rename` in `frontend/src/lib/api.ts`**

Add types and `rename` method to `api`:

```ts
export type RenameRequest = {
  glob?: string
  recursive?: boolean
  format?: string
  dry_run?: boolean
}

export type RenameItem = {
  old_path: string
  new_path: string
  source_tag: string
  old_slug: string | null
  new_slug: string | null
}

export type RenameSkippedItem = {
  path: string
  reason: "already_named" | "no_creation_date" | "workspace_locked" | "error"
  detail?: string | null
}

export type RenameCounts = {
  renamed: number
  workspaces_synced: number
  skipped_already_named: number
  skipped_no_date: number
  skipped_locked: number
  skipped_error: number
}

export type RenameReport = {
  dry_run: boolean
  format: string
  renamed: RenameItem[]
  skipped: RenameSkippedItem[]
  warnings: string[]
  counts: RenameCounts
}
```

And on `export const api = { ... }`:
```ts
  rename: (body: RenameRequest = {}) =>
    jsonRequest<RenameReport>("POST", "/api/rename", body),
```

- [ ] **Step 2: Create `frontend/src/views/RenameDialog.tsx`**

Build `RenameDialog` matching `CollectDialog.tsx`'s styling and UX:
- Inputs: Filename filter (`glob`, placeholder `All videos (e.g. IMG*)`), Timestamp format (`format`, default `%Y-%m-%d %H-%M-%S`), and `Include subfolders` (`recursive` checkbox).
- Debounced (250 ms) dry-run preview via `api.rename({ glob, recursive, format, dry_run: true })` whenever opened or inputs change.
- Summary block showing `counts.renamed`, `counts.workspaces_synced`, and skipped counts.
- Scrollable preview list (`max-h-64 overflow-y-auto`) showing each `old_path → new_path`, subtle tag pill (`QuickTime` for `com.apple.quicktime.creationdate`, `creation_time` otherwise), and workspace slug sync hint (`old_slug → new_slug`) when present.
- Collapsible skipped/warnings details if any files are skipped or warnings exist.
- Before committing (`dry_run: false`), calls `onBeforeMutateWorkspaces(affectedSlugs)` so open video proxies/drawers release file handles.
- On commit success, calls `onRenamed(report)` and closes dialog.

- [ ] **Step 3: Wire `RenameDialog` into `frontend/src/App.tsx`**

- Import `CalendarClock` from `lucide-react` and `RenameDialog` from `./views/RenameDialog`.
- Add `const [renameOpen, setRenameOpen] = useState(false)`.
- In the `{workingRoot && (...)}` toolbar block next to `Scan` and `Collect…`, add:
  ```tsx
  <Button
    variant="ghost"
    size="sm"
    onClick={() => setRenameOpen(true)}
    title="Rename source videos on disk using embedded creation-date metadata"
  >
    <CalendarClock className="h-4 w-4 mr-1.5" />
    Rename…
  </Button>
  ```
- Render `<RenameDialog>` beside `<CollectDialog>`:
  ```tsx
  <RenameDialog
    open={renameOpen}
    onOpenChange={setRenameOpen}
    onBeforeMutateWorkspaces={handleBeforeMutateWorkspace}
    onRenamed={(report) => {
      const oldSlugs = new Set(
        report.renamed.map((r) => r.old_slug).filter((s): s is string => !!s),
      )
      if (oldSlugs.size > 0) {
        setSelected((prev) => new Set([...prev].filter((s) => !oldSlugs.has(s))))
        setExpanded((prev) => (prev && oldSlugs.has(prev) ? null : prev))
      }
      const synced = report.counts.workspaces_synced
      const syncedNote = synced > 0 ? ` (${synced} workspace${synced === 1 ? "" : "s"} synced)` : ""
      setBanner(`Renamed ${report.counts.renamed} file${report.counts.renamed === 1 ? "" : "s"}${syncedNote}`)
      refetch()
    }}
  />
  ```

- [ ] **Step 4: Update `README.md` CLI Commands table**

Add a row to the `## CLI Commands` table in `README.md`:
```markdown
| `vsplit rename [PATH]` | Rename source videos by embedded creation date (`--glob`, `--recursive`, `--format`, `--dry-run`) |
```

- [ ] **Step 5: Run full verification suite**

Run:
```powershell
Set-Location frontend; bun run build; bun run lint; bunx tsc -b; Set-Location ..
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
Expected: All frontend builds/lint/typechecks and all Rust fmt, clippy, and workspace tests PASS with zero warnings or errors.

- [ ] **Step 6: Commit**

```powershell
git add frontend/src/lib/api.ts frontend/src/views/RenameDialog.tsx frontend/src/App.tsx README.md
git commit -m "feat(ui): add RenameDialog with live creation-date preview and workspace sync"
```
