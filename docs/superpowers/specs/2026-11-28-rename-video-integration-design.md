# Creation-Date Source Video Renaming (`vsplit rename`) Design

**Date:** 2026-11-28
**Status:** Draft for review

## Goal

Integrate the standalone `rename-video` utility into `vsplit` as a first-class, previewable source-video renaming tool across `vsplit-core`, `vsplit-cli`, `vsplit-server`, and the Desktop UI.

Users can rename raw camera/iPhone video clips on disk to their embedded capture timestamp (default `YYYY-MM-DD HH-MM-SS.<ext>`) either **before** or **after** scanning a folder into `.vsplit/`, with deterministic duplicate handling, strict idempotency, dry-run previewing, and lock-safe synchronization of existing `.vsplit/` workspaces.

---

## Non-Goals

- Renaming already-rendered split outputs in `.vsplit/workspaces/<slug>/output/` or changing the `plan`/`split` output filename template.
- Automatically mutating source filenames during `vsplit scan` or folder drop.
- Adding mandatory new fields to `config.yaml` (existing v1 `config.yaml` files remain unchanged).

---

## Architecture Overview

A single two-phase Rust engine in `crates/vsplit-core/src/rename.rs` serves both CLI and Desktop:

```text
  vsplit-cli (vsplit rename) ──┐
                               ├──▶ vsplit-core::rename (plan_renames / apply_rename_plan)
  vsplit-server (POST /api/rename) ──┘       ├── toolchain (ffprobe) + cached probe/ffprobe.json
        ▲                                    ├── globset (optional filename filter)
        │                                    ├── locks (index.lock + per-workspace lock)
  frontend (RenameDialog)                    └── index / manifest / slugify
```

1. **Plan phase (read-only):** Discovers candidate clips, extracts capture timestamps, computes deterministic target filenames and workspace slug updates, detects locked workspaces, and returns a `RenamePlan` / `RenameReport` with `dry_run: true`.
2. **Apply phase (mutating):** Acquires `.vsplit/.locks/index.lock` (when `.vsplit/` is present), acquires each affected workspace lock in canonical order, renames files on disk (with two-step temp-file swap for cycles/case-only renames), syncs workspace directories + `manifest.json` + `index.json`, and returns a `RenameReport` with `dry_run: false`.

---

## Section 1: Core Engine (`crates/vsplit-core/src/rename.rs`)

### Options & Data Types

```rust
pub const DEFAULT_RENAME_FORMAT: &str = "%Y-%m-%d %H-%M-%S";

#[derive(Debug, Clone)]
pub struct RenameOptions {
    pub recursive: Option<bool>,   // defaults to config.scan.recursive (false)
    pub glob: Option<String>,      // optional case-insensitive filename glob (e.g. "IMG*")
    pub format: Option<String>,    // defaults to DEFAULT_RENAME_FORMAT
    pub dry_run: bool,
}
```

Report structs (`Serialize` for both CLI `--json` and HTTP API):

- `RenameItem`: `{ old_path, new_path, source_tag, old_slug, new_slug }`
  - `old_path` and `new_path` are POSIX-normalized paths relative to `working_root`.
  - `source_tag` is `"com.apple.quicktime.creationdate"` or `"creation_time"`.
  - `old_slug` / `new_slug` are `Option<String>` (populated when linked to an existing workspace).
- `RenameSkipReason`: `AlreadyNamed | NoCreationDate | WorkspaceLocked | Error` (`snake_case`).
- `RenameSkippedItem`: `{ path, reason, detail: Option<String> }`.
- `RenameCounts`: `{ renamed, workspaces_synced, skipped_already_named, skipped_no_date, skipped_locked, skipped_error }`.
- `RenameReport`: `{ dry_run, format, renamed: Vec<RenameItem>, skipped: Vec<RenameSkippedItem>, warnings: Vec<String>, counts: RenameCounts }`.

### Format Validation

Before probing any files, validate `format`:
- Reject empty/whitespace-only strings.
- Reject forbidden filename characters: `/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`, and ASCII control characters (`0x00..=0x1F`).
- Test-format a fixed sample timestamp (`2026-05-01T14:23:05+08:00`) using `chrono::format::StrftimeItems`. Reject if formatting errors, yields an empty string, contains forbidden characters, or ends with `.` or space (invalid on Windows).

### File Discovery & Filtering

- Walk `working_root` using `config.scan.extensions` (case-insensitive) and skipping `.vsplit/` (reusing the same candidate-walk rules as `scan.rs`).
- If `opts.glob` is provided, compile it via `globset::GlobBuilder::new(pattern).case_insensitive(true).literal_separator(false).build()` and match against each candidate's filename (basename). Non-matching files are ignored (not listed as errors).
- Sort candidates deterministically by normalized relative path.

### Creation-Date Extraction

Probing runs asynchronously with bounded concurrency (`4`):

1. **Probe cache shortcut:** If the candidate matches an indexed workspace (by `source_relative_path`, verified against `size_bytes`) and `.vsplit/workspaces/<slug>/probe/ffprobe.json` exists and parses as JSON, read `format.tags` / `streams[].tags` directly without spawning `ffprobe`.
2. **Fallback `ffprobe`:** Run `ffprobe -v error -print_format json -show_format -show_streams <path>` via `vsplit_core::toolchain`.
3. **Tag priority (case-insensitive tag key lookup):**
   1. `format.tags["com.apple.quicktime.creationdate"]`
      - Parse ISO-8601 / RFC3339 with timezone offset (`DateTime::parse_from_rfc3339`, `%Y-%m-%dT%H:%M:%S%z`, `%Y-%m-%dT%H:%M:%S%.f%z`).
      - Format directly using the parsed `DateTime<FixedOffset>` so capture wall-clock time is preserved (matching original `rename-video/main.py` behavior for iPhone videos recorded across timezones).
   2. `format.tags["creation_time"]`, then first video stream `tags["creation_time"]`
      - Parse RFC3339 / ISO-8601 (`%Y-%m-%dT%H:%M:%S%.fZ`, `%Y-%m-%dT%H:%M:%S%z`, or naive `%Y-%m-%d %H:%M:%S` treated as local).
      - Convert offset-aware timestamps (typically UTC `Z`) to `chrono::Local` before formatting.
4. If `ffprobe` fails on a single corrupt file, record `RenameSkipReason::Error` with stderr detail and continue processing the rest. If neither tag exists or parses, record `RenameSkipReason::NoCreationDate`.

### Idempotency & Duplicate Numbering

Evaluated independently per parent directory:

1. **Idempotency check:** For a candidate with original filename `<stem>.<ext>` and formatted timestamp `base_stem`:
   - If `stem` equals `base_stem` (case-insensitive), OR `stem` matches `<base_stem>(<n>)` where `<n>` is a positive integer (`>= 1`), mark as `RenameSkipReason::AlreadyNamed`.
2. **Occupied filename set:** Start with lowercase filenames of **all** existing entries in that parent directory, then remove the lowercase filenames of candidates that will actually be renamed in this batch (all non-video files, non-matching files, skipped files, and `already_named` files remain occupied).
3. **Target assignment (in sorted order):**
   - Preserve the candidate's original extension verbatim (`ext`).
   - Try `<base_stem>.<ext>`.
   - If its lowercase key is in `occupied`, try `<base_stem>(1).<ext>`, `<base_stem>(2).<ext>`, … until free.
   - Insert the chosen lowercase key into `occupied`.
4. **Cycle / case-only rename safety:** During execution, if a planned `new_path` currently exists on disk (possible when renaming A → B and B → C in the same batch, or changing filename case on Windows/macOS), execute a two-phase rename via a unique temp file `.vsplit-rename-tmp-<hex>-<name>` in the same directory.

### `.vsplit/` Workspace Sync & Locking

- **Lookup:** Load `index.json` (if present). Match each candidate to an existing workspace by `source_relative_path == old_rel`, falling back to `partial_hash` if needed.
- **Locking:**
  - During planning (`dry_run: true`): check `inspect_workspace_lock(working_root, &old_slug)`. If locked, record `RenameSkipReason::WorkspaceLocked` (with actor/pid/stage detail) and keep the file's current name in `occupied`.
  - During apply (`dry_run: false`): acquire `lock_index(working_root)` before mutating any workspace, then acquire `try_acquire_workspace_lock(working_root, &old_slug, LockLease::new(actor, "rename", run_id))` per workspace-backed item (preserving canonical lock order: `index.lock` before `<slug>.lock`). If a lock was acquired by another process between planning and execution, skip that item with `WorkspaceLocked` and do not rename its source file.
- **Slug update:**
  - Compute `base_slug = slugify(&new_filename)`.
  - If `base_slug == old_slug`, keep `old_slug` and update `source_relative_path` only.
  - Otherwise resolve `new_slug = resolve_collision(&base_slug, &live_slugs_without_old, &retired_slugs)`.
  - Do **not** add `old_slug` to `retired_slugs` (the workspace was renamed, not deleted).
  - Rename source file on disk (`old_path` → `new_path`). If the source file rename fails, record `RenameSkipReason::Error` and leave `.vsplit/` untouched.
  - Rename workspace directory `.vsplit/workspaces/<old_slug>` → `.vsplit/workspaces/<new_slug>`, update `manifest.id = new_slug` and `manifest.source.relative_path = new_rel`, save `manifest.json` atomically, and re-key the entry in `index.json`.
  - **Windows external-handle fallback:** If renaming `.vsplit/workspaces/<old_slug>` fails (e.g., `PermissionDenied` because another program holds an open handle inside that directory), keep `old_slug`, still update `source_relative_path` in `manifest.json` and `index.json`, and append a warning to `report.warnings` so disk files and `.vsplit/` never desync.
  - Existing `plan/cuts-resolved.json` and `output/` files (if `split` already ran) are left intact so `verify` and `collect` continue to work; future `plan`/`split` runs automatically use the updated `manifest.id`.
- Save `index.json` atomically once all items complete.

---

## Section 2: CLI & HTTP API

### CLI: `vsplit rename`

Add `crates/vsplit-cli/src/commands/rename.rs` and `Command::Rename` in `crates/vsplit-cli/src/main.rs`:

```text
vsplit [GLOBAL OPTIONS] rename [PATH] [--glob PATTERN] [--recursive | --no-recursive] [--format STRFTIME] [--dry-run]
```

- `[PATH]` optionally overrides `-w / --working-dir` (matching `vsplit scan [PATH]`).
- `--dry-run` prints the full rename plan without modifying files or `.vsplit/`.
- `--json` emits the standard `{"ok": true, "data": RenameReport}` envelope.
- Exit codes:
  - `0`: success (including dry-run or zero renames)
  - `2`: invalid `--glob`, invalid `--format`, or target path is not a directory
  - `4`: `ffprobe` not found (`MISSING_DEP`)
  - `1`: unexpected runtime I/O error

### HTTP API: `POST /api/rename`

Add `POST /api/rename` to `crates/vsplit-server/src/routes/mod.rs` (handler in `crates/vsplit-server/src/routes/actions.rs`):

- **Request JSON:**
  ```json
  {
    "glob": "IMG*",
    "recursive": false,
    "format": "%Y-%m-%d %H-%M-%S",
    "dry_run": true
  }
  ```
  All fields optional (`dry_run` defaults to `false`).
- **Validation errors:** Return `422 Unprocessable Entity` with `{"detail": "..."}` on invalid `glob` or `format`; `503 Service Unavailable` if `ffprobe` is missing.
- **Watcher handoff (Windows safety):** Add `WorkspaceWatcher::unwatch_workspaces(&mut self, working_root: &Path, slugs: &[String])` and `AppState::unwatch_workspaces(&self, slugs: &[String])`. When `dry_run == false`:
  1. Compute plan to identify affected `old_slug`s.
  2. Call `state.unwatch_workspaces(&affected_slugs)` to release directory-watch handles on `.vsplit/workspaces/<old_slug>` and its `decisions/` subdirectory.
  3. Apply the rename plan.
  4. Call `state.sync_watches()` so the watcher monitors the renamed workspace directories and `run_watch_pump` emits `WorkspacesAdded` / `WorkspacesRemoved` SSE events automatically.
- **Response JSON:** Serialized `RenameReport`.

---

## Section 3: Desktop UI (`RenameDialog`)

### Toolbar Entry Point (`frontend/src/App.tsx`)

- Add a `Rename…` ghost button (with `CalendarClock` icon) next to `Scan` and `Collect…` whenever `workingRoot` is open.
- Pass `onBeforeMutateWorkspaces={handleBeforeMutateWorkspace}` and `onRenamed` callback to `RenameDialog` so:
  - Before committing a rename, any open inline video preview (`previewSlug`) or Cuts Drawer (`cutsSlug`) for affected slugs is closed cleanly (releasing proxy media handles on Windows).
  - After committing, renamed slugs are removed from `selected`/`expanded`, a summary banner is shown (`Renamed N files (M workspaces synced)`), and `refetch()` refreshes the list.

### Dialog UX (`frontend/src/views/RenameDialog.tsx`)

Follows the `CollectDialog.tsx` live-preview pattern:
- **Inputs:**
  - Filename filter (`Input`, optional, placeholder `All videos (e.g. IMG*)`)
  - Timestamp format (`Input`, default `%Y-%m-%d %H-%M-%S`)
  - `Include subfolders` (`Checkbox`, defaults to `false`)
- **Live dry-run preview** (debounced ~250 ms whenever the dialog opens or inputs change):
  - Summary banner with counts (`N files will be renamed`, `M workspaces synced`, and skipped breakdown).
  - Scrollable table/list of planned renames: `old_path → new_path`, subtle tag badge (`QuickTime` / `creation_time`), and workspace slug sync badge (`img-0042 → 2026-05-01-14-23-05`) when applicable.
  - Collapsible skipped/warnings summary when items are skipped (`no creation date`, `already named`, `locked`, `error`).
- **Actions:** `Cancel` and `Rename N` (disabled when `renamed.length === 0` or busy).

### API Client (`frontend/src/lib/api.ts`)

Add typed `RenameRequest` / `RenameReport` types and `api.rename(body)` calling `POST /api/rename`.

---

## Verification & Testing Plan

1. **Core unit tests (`crates/vsplit-core/src/rename.rs`):**
   - Tag parsing (`com.apple.quicktime.creationdate` with `+0800`, `+08:00`, `-05:00`, `Z`; fallback to `creation_time`; stream-level fallback; case-insensitive tag keys; invalid/missing tags).
   - Format string validation (rejects `:`, `/`, `\`, `*`, `?`, `"`, `<`, `>`, `|`, empty, invalid `%` directive, trailing `.`/space).
   - Idempotency & duplicate numbering (`T.mov`, `T(1).mov`, `T(2).mov`; re-running skips already-named `T.mov` and `T(1).mov`; preserves `.MOV`/`.mp4` extension; case-insensitive collision safety; cycle/case-only two-step swap).
2. **Core integration tests (`crates/vsplit-core/tests/rename_videos.rs`):**
   - Real FFmpeg-generated MP4/MOV fixtures with `com.apple.quicktime.creationdate` and `creation_time` metadata:
     - Unadopted folder dry-run (leaves disk untouched) and apply (renames files).
     - Adopted workspace sync: renames file + `.vsplit/workspaces/<old-slug>` → `<new-slug>`, preserves `manifest.stages` and `decisions/cuts.json`, updates `index.json` + `manifest.source.relative_path`.
     - Locked workspace skipped cleanly while unlocked siblings rename.
     - Cached `probe/ffprobe.json` shortcut works.
     - `--glob "IMG*"` and `--recursive` behavior.
3. **CLI contract tests (`crates/vsplit-cli/tests/cli_contract.rs`):**
   - `vsplit --json rename --dry-run` and apply.
   - Invalid `--format` exits with code `2`.
4. **Server integration test (`crates/vsplit-server/tests/`):**
   - `POST /api/rename` dry-run + apply while `WorkspaceWatcher` is active, verifying `/api/workspaces` reflects the renamed slug without Windows lock errors.
5. **Repo-wide checks:**
   - `cd frontend && bun run build && bun run lint && bunx tsc -b`
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
