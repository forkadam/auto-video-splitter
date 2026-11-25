# vsplit Native Desktop App & AI-Agent CLI

*   **Author:** forkadam
*   **Status:** Approved
*   **Date:** 2026-11-25

--------------------------------------------------------------------------------

## Overview

`vsplit` is a local, scene-aware video splitter that scans folders of personal video clips, detects scene cuts, renders thumbnail and proxy previews, pauses at a human review gate (`cuts.json`), and splits kept segments without re-encoding via FFmpeg. Today it runs as a Python CLI (`vsplit`) paired with a localhost FastAPI server and a React 19 / Vite 8 / Tailwind v4 / shadcn dashboard (`vsplit dashboard`).

This design turns `vsplit` into a standalone desktop application for Windows and Linux (targeting Omarchy: Arch Linux with Hyprland on Wayland) while keeping a full CLI designed for AI-agent automation. Both interfaces operate on the same `.vsplit/` workspace directory with live two-way synchronization, `< 1.0 s` desktop startup, `< 50 ms` metadata CLI startup, `< 50 MB` idle backend memory, and one-click updates from GitHub Releases. External `ffmpeg` and `ffprobe` binaries remain user-provided or configured post-install rather than bundled into every app update.

## The Problem

Running `vsplit` today creates five operational problems for a daily desktop and AI-agent workflow across Windows and Arch/Omarchy Linux:

1. **Terminal-bound startup and single-folder lock-in:** Launching the dashboard requires opening a terminal, pointing `vsplit -w <folder> dashboard` at one fixed folder, and opening a browser tab. There is no native folder picker, recent-projects switcher, drag-and-drop onboarding, or in-app `scan` trigger—the empty UI instructs the user to drop back to a terminal and run `vsplit scan`.
2. **Unsafe concurrent execution between GUI and AI agents:** The current `.vsplit/` persistence layer assumes one active process. `_recover_running()` in `src/vsplit/core/orchestrator.py` treats any stage marked `running` on disk as a crashed run and deletes its output subdirectory (`shutil.rmtree`), so an AI agent invoking `vsplit run` while the GUI runs a stage corrupts active work. `index.json` and `manifest.json` have unlocked read-modify-write races, and `ManifestWatcher` only polls `index.json` and `manifest.json` every 0.5 seconds (missing agent edits to `cuts.json` and `config.yaml`).
3. **CLI is not safe or structured for AI-agent automation:** Only `status` honors the global `--json` flag—and it dumps full manifest payloads (estimated 20k–60k LLM tokens for a 100-clip batch). `reset` and `clean` pass a no-op stage filter predicate that silently turns `--filter stage=<name>` into a match-all wildcard; `approve` ignores `stage=`; `run` exits `0` even when stages fail; `scan`, `reset --all`, and `clean` hang headless agents on interactive confirmation prompts; and `ignore`/`unignore`, validated `cuts` editing, and `logs` have no CLI commands.
4. **Detached desktop blind spots and Windows/Linux incompatibilities:** `PreviewStage` and `SplitStage` invoke `ffmpeg` without capturing `stderr`, so failure causes disappear without a terminal. Subprocess calls omit Windows `CREATE_NO_WINDOW`, flashing console windows on every probe, thumbnail, and split. Long-running dashboard actions create an unreachable local `threading.Event()`, leaving jobs uncancellable and child `ffmpeg` processes orphaned on exit. `scan` persists Windows backslashes (`\`) into `.vsplit/` relative paths, breaking cross-OS sharing with Arch/Omarchy, and Windows `platformdirs` resolves user config to `%LOCALAPPDATA%\vsplit\vsplit\config.yaml` rather than a unified cross-tool location.
5. **Distribution and update overhead:** Keeping the GUI and CLI aligned across Windows and Arch/Omarchy requires manual `uv` updates and environment maintenance, with no one-click GitHub Release update flow and no preflight check for missing `ffmpeg`/`ffprobe` or Linux codec dependencies.

## Goals

*   **Desktop usability on Windows and Arch/Omarchy (Hyprland Wayland):** Replace the browser-based `vsplit dashboard` with a dedicated desktop window featuring native folder picking, recent-folder switching, drag-and-drop for folders and single video files, in-app `scan`, live job cancellation, and HTTP 206 `<video>` proxy scrubbing on both Windows WebView2 and Arch WebKitGTK.
*   **AI-agent-ready CLI parity:** Provide uniform `--json` output across every CLI subcommand (with compact `status` summaries by default and `--full` opt-in), documented non-zero exit codes, non-interactive fail-fast behavior when stdin is not a TTY, unified `--filter` enforcement across all commands, and CLI commands for `ignore`/`unignore`, validated `cuts` get/set, `logs`, and `doctor`.
*   **Crash-safe live two-way sync over `.vsplit/`:** Prevent concurrent cooperating GUI and CLI/agent mutations from corrupting manifests, indexes, or in-flight stage directories, and reflect external edits to `index.json`, `manifest.json`, `decisions/cuts.json`, and `.vsplit/config.yaml` in the open UI within 1 second without overwriting unsaved editor changes.
*   **Startup, latency, and memory targets:** Achieve desktop window cold start under 1.0 s, metadata CLI cold start under 50 ms (up to 200 workspaces on local SSD), subprocess-inclusive `doctor --json` under 500 ms, idle backend memory under 50 MB (excluding WebView), zero Windows console-window flashes, and complete child `ffmpeg`/`ffprobe` termination on cancel or exit.
*   **Co-versioned one-click GitHub updates:** Detect new GitHub Releases and update both desktop app and CLI in lockstep in one click on Windows (NSIS) and Arch/Omarchy (Tauri v2 AppImage + extracted `~/.local/bin/vsplit` shim).
*   **Shared external FFmpeg/FFprobe discovery:** Resolve `ffmpeg` and `ffprobe` identically across GUI and CLI (environment override, explicit path in `app.toml`, or system `PATH`), verify codec readiness via `doctor`, and run fully offline after setup.
*   **v1 workspace backward compatibility and cross-OS portability:** Read and write `.vsplit/` v1 workspaces without breaking rollback to `vsplit 0.3.0` (which enforces Pydantic `extra="forbid"` unknown-field rejection across all `.vsplit/` schemas) by storing app-only settings outside `.vsplit/`, and normalize stored relative paths to POSIX `/` across Windows and Linux.

## Non-Goals

*   Multi-user access, LAN/remote hosting, user accounts, or cloud workspace syncing.
*   Retaining legacy browser-hosted `vsplit dashboard` mode in the Rust CLI (`vsplit-desktop` is the sole UI).
*   Bundling `ffmpeg` and `ffprobe` inside every app update artifact.
*   Rewriting the existing React 19 / Tailwind v4 / shadcn UI in a native widget toolkit (Qt, egui, or Slint).
*   Linking C/C++ `opencv-rust` or `libavcodec`/`ffmpeg-next` dynamic libraries into Rust binaries.
*   Maintaining two full backends (Python and Rust) in parallel after Milestone 4 completes.

## Open Questions

Two empirical unknowns remain to be measured during the Milestone 0 feasibility spike before full implementation:

*   **Spike Unknown 1 (Hyprland / WebKitGTK AppImage behavior on Omarchy):** Whether the target Arch/Omarchy GPU stack requires `WEBKIT_DISABLE_DMABUF_RENDERER=1` or extra GStreamer packages (`gst-libav`, `gst-plugins-good`/`bad`) for H.264 `<video>` scrubbing inside a Tauri v2 AppImage, and whether AppImage-bundled `libwayland-client` conflicts with Arch rolling Mesa.
*   **Spike Unknown 2 (VFR iPhone `.MOV` detector parity):** Exact frame-timestamp delta between the Rust FFmpeg-pipe scorers (`content`, `adaptive`, `histogram`, `hash`) and PySceneDetect on 10-bit HEVC variable-frame-rate `.MOV` clips (target: within +/-2 frames / +/-0.08 s on canonical thresholds).

## Critical User Journeys (CUJs)

### CUJ 1: Cold-Launch Desktop Folder Review & Lossless Split

*   **Given**: `vsplit` desktop app is installed on Windows or Arch/Omarchy (Hyprland Wayland), `ffmpeg`/`ffprobe` are available via `PATH` or configured in `app.toml`, and a folder contains raw `.MOV`/`.MP4` clips (including 10-bit HEVC iPhone footage).
*   **When**: The user launches the desktop app from the Start Menu or Hyprland launcher, picks or drops the folder, clicks **Scan**, runs `probe → detect → preview → review`, scrubs the video preview, edits `keep`/`label`/`merge_with_next` in the inline Cuts Drawer, and clicks **Save & Continue**.
*   **Then**: The app opens in under 1.0 s with zero Windows console-window flashes, scans without terminal commands, generates thumbnails and playable `source-proxy.mp4` files (scrubbing with zero `MediaError` events on WebView2 and WebKitGTK), splits kept clips into `output/` without re-encoding, verifies output durations, and cancels any in-flight run on demand without leaving orphan `ffmpeg` processes.

### CUJ 2: AI Agent Automating CLI While Desktop GUI Stays Open

*   **Given**: The desktop app is open on a working folder, and an AI agent invokes `vsplit` CLI commands (`scan --json`, `status --json`, `run --through preview --json`, `cuts get`/`cuts set --expected-hash`, `approve`, `run --json`) from a headless shell (non-TTY stdin).
*   **When**: The agent executes commands sequentially—or attempts a mutating command while the user is actively running a stage or editing `cuts.json` in the GUI.
*   **Then**: Each metadata CLI command returns compact valid JSON in under 50 ms without hanging on interactive prompts; stage failures return exit code `1` with captured `ffmpeg` stderr; the open GUI updates within 1 second on manifest, `cuts.json`, and `config.yaml` changes; unsaved GUI cuts edits display a Reload / Keep Mine conflict banner instead of being overwritten; and concurrent runs on the same workspace fail fast with exit code `3` / HTTP `409` without triggering false crash recovery or deleting active stage outputs.

### CUJ 3: One-Click Update from GitHub Releases (GUI + CLI in Lockstep)

*   **Given**: The user runs version `vX.Y.Z` of `vsplit` desktop and `vsplit` CLI on Windows or Arch/Omarchy, and version `vX.Y.(Z+1)` is published to GitHub Releases with signed metadata.
*   **When**: The app checks the static GitHub Release manifest on launch (or manual check), shows an update indicator, and the user clicks **Update & Restart**.
*   **Then**: The updater verifies the Ed25519 signature, blocks updates while desktop `JobCoordinator` runs are active, updates both binaries in lockstep (via NSIS on Windows, displaying a retry prompt if an external `vsplit.exe` process holds the file open; via AppImage replacement followed by atomic `~/.local/bin/vsplit` symlink extraction on first post-update launch on Linux), and restarts with matching GUI and CLI versions.

### CUJ 4: First-Run Preflight & Offline Operation Without Bundled FFmpeg

*   **Given**: The user installs `vsplit` on a clean Windows or Arch/Omarchy machine where `ffmpeg`/`ffprobe` (or Linux GStreamer H.264 plugins) are missing or outside the detached GUI's `PATH`.
*   **When**: The user opens the desktop app or runs `vsplit doctor --json`.
*   **Then**: Both GUI and CLI run the same preflight check, report exact missing tools or encoders (`ffmpeg`, `ffprobe`, `libx264`, `aac`, or GStreamer decoders on Linux) with OS setup instructions and binary file pickers, persist any configured tool path in `app.toml` outside `.vsplit/` so both GUI and CLI share it, and run all pipeline stages fully offline once configured.

## Technical Plan (Architecture)

*Terminology note (full v1 CLI and schema details are documented in `README.md`):*
*   `one` / "`one`-style adoption": adopts a single dropped video file into its parent folder's `.vsplit/` workspace without scanning sibling files.
*   `collect` / `collect_outputs`: gathers final split clips from every `split=done` workspace's `output/` folder into one destination directory (`copy`, `move`, or `link`).
*   `relocate_policy`: controls how `scan` handles a source file whose content hash matches an existing workspace at a different path (`auto` updates the pointer, `skip` leaves it unchanged, `report` lists mismatches without writing).
*   `awaiting_user`: the `review` stage status when `decisions/cuts.json` has been written and awaits user edits or `approve`.
*   `extra="forbid"`: Pydantic strict-schema mode in `vsplit 0.3.0` that rejects any unrecognized field in `.vsplit/` JSON/YAML files.

### 1. System Topology & Cargo Workspace

The backend, CLI, and desktop shell move into a single Rust Cargo workspace alongside the existing React 19 / Vite 8 / Tailwind v4 / shadcn frontend. Both `vsplit-desktop` (Tauri v2 GUI binary) and `vsplit` (console CLI binary) link `vsplit-core` directly; `vsplit-server` is linked exclusively by `vsplit-desktop` (keeping `vsplit-cli` free of HTTP server and web-asset dependencies). Both binaries operate directly on `.vsplit/` using shared OS file locks.

```text
+-------------------------------------------------------------------+
|  vsplit-desktop (Tauri v2, WINDOWS subsystem on Win / Wayland)    |
|  +-----------------------------+   +---------------------------+  |
|  | React 19 + Tailwind v4 UI   |   | Tauri IPC Commands        |  |
|  | (api.ts, sse.ts, iframe)    |<->| - bootstrap (url + token) |  |
|  |                             |   | - folder/file picker, DnD |  |
|  |   REST + SSE + HTTP 206     |   | - app.toml CRUD, updater  |  |
|  +--------------+--------------+   | - CLI shim sync / doctor  |  |
|                 |                  +---------------------------+  |
|                 v                                                 |
|  +-------------------------------------------------------------+  |
|  | vsplit-server (in-process Axum on 127.0.0.1:0 + token auth) |  |
|  | - REST /api/*  - SSE /m/{token}/events                      |  |
|  | - HTTP 206 /m/{token}/... media & thumbs  - ActiveProject   |  |
|  +------------------------------+------------------------------+  |
|                                 |                                 |
+---------------------------------|---------------------------------+
                                  v
+-------------------------------------------------------------------+
|  vsplit-core (shared Rust library)                                |
|  - v1 .vsplit/ schemas & atomic I/O (POSIX relative paths)        |
|  - OS locks (flock / LockFileEx) + .lease.json + crash recovery   |
|  - notify debounced watcher (index, manifest, cuts, config, lock) |
|  - JobCoordinator (CancellationToken + Job Objects / pgroup)      |
|  - MediaToolchain (cached ffmpeg/ffprobe + CREATE_NO_WINDOW)      |
|  - 7 stages + FFmpeg-pipe 4-detector engine (content/adaptive/    |
|    histogram/hash) + headless scan / actions / collect / doctor   |
+---------------------------------+---------------------------------+
                                  ^
                                  | direct calls (no HTTP server)
+---------------------------------|---------------------------------+
|  vsplit (CLI binary, CONSOLE subsystem on Win, <50ms cold start)  |
|  - Uniform --json output       - Deterministic exit codes         |
|  - Non-interactive TTY guard   - ignore/unignore/cuts/logs/doctor |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|  Shared On-Disk State (<working_root>/.vsplit/)                   |
|  - index.json, config.yaml (v1 frozen schema)                     |
|  - workspaces/<slug>/{manifest.json, decisions/cuts.json, ...}    |
|  - .locks/{index.lock, <slug>.lock, <slug>.lease.json}            |
+-------------------------------------------------------------------+
```

### 2. Shared On-Disk Contract, OS Locking & Two-Way Sync

To prevent concurrent write races between cooperating `vsplit-desktop` and `vsplit` CLI processes while preserving backward compatibility with `vsplit 0.3.0`:

*   **Frozen v1 `.vsplit/` schemas:** `index.json`, `config.yaml`, `manifest.json`, `decisions/cuts.json`, `detect/scenes.json`, and `plan/cuts-resolved.json` retain their v1 fields and formats. All new desktop-only state (`recent_folders`, `last_folder`, explicit `ffmpeg_path`/`ffprobe_path`, `wayland_disable_dmabuf`) is stored in a separate machine-local file outside `.vsplit/`:
    *   Linux: `$XDG_CONFIG_HOME/vsplit/app.toml` (default `~/.config/vsplit/app.toml`).
    *   Windows: `%APPDATA%\vsplit\app.toml`.
    *   User-global pipeline config (`config.yaml`) unifies on `~/.config/vsplit/config.yaml` (Linux) and `%APPDATA%\vsplit\config.yaml` (Windows), with read-only fallback to the legacy Windows `platformdirs` path (`%LOCALAPPDATA%\vsplit\vsplit\config.yaml`) when `%APPDATA%\vsplit\config.yaml` does not exist; all writes go to `%APPDATA%\vsplit\config.yaml`. The global CLI `--config <path>` flag overrides pipeline `config.yaml` only, never `app.toml`.
*   **Cross-OS POSIX relative paths:** All persisted relative paths (`source_relative_path`, `source.relative_path`, `error.log_path`) are canonicalized to `/` on write and normalized (`\` -> `/`) on read so workspaces open identically on Windows and Arch/Omarchy.
*   **Windows-safe OS locks & sidecar lease metadata (`.vsplit/.locks/`):**
    *   `.vsplit/.locks/index.lock`: Short-lived exclusive OS file lock (`flock` on Linux, `LockFileEx` on Windows via `fs2`) held around every `index.json` read-modify-write transaction (`scan`, `clean`, `one`).
    *   `.vsplit/.locks/<slug>.lock` + `.vsplit/.locks/<slug>.lease.json`: Because Windows `LockFileEx` enforces mandatory read locks (blocking peer reads with `ERROR_LOCK_VIOLATION`), the exclusive OS lock is held on an empty sentinel file `.vsplit/.locks/<slug>.lock` (never unlinked while held to avoid Linux inode split-brain), while owner metadata (`{pid, actor: "desktop" | "cli", stage, run_id, started_at, version}`) is atomically written to the sibling `.vsplit/.locks/<slug>.lease.json`. Peers treat `<slug>.lease.json` as active only when `try_lock_exclusive()` on `<slug>.lock` fails.
    *   **Unified workspace lock scope & ordering:** All mutating workspace operations (`run`, `reset`, `clean`, `approve`, `ignore`/`unignore`, and `cuts set`) acquire `.vsplit/.locks/<slug>.lock` exclusively (paired with an in-process mutex map inside `vsplit-desktop` for intra-process thread exclusion). Whenever an operation touches both index and workspace (`clean`), it acquires `index.lock` before `<slug>.lock` to prevent deadlocks. If `<slug>.lock` is held, peer mutating commands fail fast with exit code `3` / HTTP `409 Conflict` including the owner's `actor`, `pid`, and `stage`.
    *   **Lease-aware crash recovery:** Pre-dispatch crash recovery only recovers a stage marked `running` in `manifest.json` after acquiring `.vsplit/.locks/<slug>.lock` exclusively—proving no live peer process owns it—and writes `manifest.json` only when a stage state changed.
*   **OS-native debounced watching (`notify`):** `vsplit-server` replaces the 500 ms polling loop with a debounced (~100 ms per-path) `notify` watcher scoped non-recursively to `.vsplit/`, `.vsplit/.locks/`, and each workspace's root and `decisions/` directory (explicitly excluding `preview/images/` and `output/` to prevent event storms and Linux `inotify` exhaustion, with automatic polling fallback on `ENOSPC`). On broadcast overflow, the server emits a `sync_lagged` event prompting a full list refresh. External edits to `cuts.json` while the Cuts Drawer has unsaved edits display a Reload / Keep Mine conflict banner (backed by `expected_hash`, the SHA-256 hex digest of raw `cuts.json` bytes, returning HTTP `409` on mismatch); external edits to `config.yaml` reload untouched fields in the Config Drawer and prompt before discarding unsaved textarea edits.

### 3. FFmpeg-Pipe Four-Detector Engine & Media Toolchain

All seven pipeline stages (`probe`, `detect`, `preview`, `review`, `plan`, `split`, `verify`) execute in `vsplit-core` with zero C++/OpenCV or `libav*` FFI dependencies:

*   **Centralized `MediaToolchain` resolver:** Both `vsplit-desktop` and `vsplit` resolve `ffmpeg` and `ffprobe` in one deterministic 3-tier order:
    1. Environment overrides (`VSPLIT_FFMPEG_PATH`, `VSPLIT_FFPROBE_PATH`).
    2. Explicit paths in machine-local `app.toml`.
    3. System `PATH`.
    Version strings and encoder capabilities (`libx264`, `aac`) are probed once per session and cached (eliminating two subprocess spawns per stage execution, or 700 spawns across a 50-clip × 7-stage batch).
*   **Subprocess hygiene & cancellation:** Every `ffmpeg`/`ffprobe` spawn goes through one runner that:
    *   Sets `CREATE_NO_WINDOW` on Windows so detached GUI runs produce zero console-window flashes.
    *   Concurrently drains `stdout` and `stderr` on async Tokio tasks, capturing bounded `stderr` (last 16 KB) alongside `argv`, exit code, and duration into `logs/<stage>.log` (with `run_id` separators and rotation) and `StageError`.
    *   Registers every child in an active `JobCoordinator` bound to a Windows Job Object or POSIX process group (`setpgid` + `SIGTERM`/`SIGKILL`), terminating active `ffmpeg` trees when the user clicks **Cancel**, switches folders, or exits the app.
*   **Rust FFmpeg-pipe scene detection (`detect` stage):**
    *   Spawns external `ffmpeg` with `-fps_mode passthrough` (`-vsync 0`) and `-vf scale=w=min(iw\,320):-2:flags=fast_bilinear,format=rgb24 -f image2pipe -vcodec ppm pipe:1` — reducing 4K60 raw RGB24 bandwidth from ~1.5 GB/s (~25 MB/frame) to ~10.4 MB/s (~170 KB/frame) while reusing FFmpeg's native 10-bit HEVC decode and auto-rotation, and using PPM's trivial `P6 <W> <H> 255\n` header so Rust knows exact post-rotation frame dimensions without guesswork.
    *   Aligns decoded frame timestamps using presentation-order PTS extracted via `ffprobe -v error -select_streams v:0 -show_entries frame=best_effort_timestamp_time -of csv=p=0` (avoiding B-frame DTS/PTS reordering and CFR drift on variable-frame-rate iPhone `.MOV` clips), converting `min_scene_length` via `max(1, int(min_scene_length * (fps or 24)))`.
    *   Runs all four scorers in pure Rust matching live PySceneDetect algorithms:
        *   `content`: RGB -> 8-bit HSV (OpenCV `H: 0..179`, `S/V: 0..255`) weighted mean delta (`delta_hue=1, delta_sat=1, delta_lum=1`, averaged by 3), plus `luma_only` fast path.
        *   `adaptive`: Rolling-window normalized ratio over `content` scores (`window_width=2`, `min_content_val=15.0`, honoring `luma_only`).
        *   `histogram`: 256-bin YUV Y-channel (luma) normalized histogram compared via Pearson correlation (`correl <= 1.0 - threshold`, matching live `HistogramDetector`).
        *   `hash`: 16x16 2D-DCT perceptual hash (pHash: grayscale resized to 32x32 via area averaging, max-normalized, 2D DCT, top-left 16x16 block thresholded `> median`, cut when normalized Hamming distance `/ 256.0 >= threshold`, matching live `HashDetector`).
    *   Logs detector diagnostics (`frames_decoded`, `decode_fps`, `score_min/p50/p95/max`, `threshold`) into `logs/detect.log` (keeping `detect/scenes.json` strictly v1 schema-compatible).
*   **Preview stage behavior:** Generates `source-proxy.mp4` whenever `preview.proxy_height > 0` (forcing `format=yuv420p` for 10-bit HEVC, matching v1's explicit `proxy_height: null` opt-out and avoiding broken direct-source iframe paths). Extracts per-scene thumbnails sequentially within each workspace (bounded globally by workspace-level `concurrency.preview`, avoiding nested concurrency multiplication) and renders `preview/index.html` via `minijinja` using pre-formatted template fields (`id_padded`, `duration_str`, `drift_str`).

### 4. Embedded `axum` Loopback Server & Desktop Shell

*   **Dynamic `ActiveProject` session:** Replaces the immutable startup `working_root`. `vsplit-desktop` opens to `last_folder` (or a recent-folders / folder-picker empty state), switches folders at runtime (rejecting with HTTP `409` or prompting to cancel if jobs are active, tearing down the old `notify` watcher, unmounting the preview `<iframe>`, and binding the new root), and handles OS drag-and-drop for folders (switch + scan prompt) and single video files (`one`-style adoption into the file's parent folder).
*   **Embedded `axum` loopback server (`127.0.0.1:0`):**
    *   Runs strictly inside `vsplit-desktop` under a single Tauri auth model. Binds an OS-assigned ephemeral port on `127.0.0.1` at startup and generates a 256-bit random `session_token` (in local Vite HMR development, an optional `VSPLIT_DEV_TOKEN` env var aligns the dev server and backend).
    *   React retrieves `{ baseUrl, token }` once via a Tauri `invoke("get_server_bootstrap")` call.
    *   All `/api/*` requests require `X-Vsplit-Token: <token>` and `Origin` matching the Tauri webview origin (rejecting cross-origin browser CSRF). Media URLs (`<video>`, `<img>`, preview `<iframe>`) and SSE (`EventSource`, which cannot attach custom headers) authenticate via path-scoped session routes (`/m/{token}/workspaces/{slug}/...` and `/m/{token}/events`) so relative links inside `preview/index.html` work without cross-origin cookies, serving HTTP `206 Partial Content` range responses and suppressing expected client-aborted seek (`BrokenPipe`/`ConnectionReset`) noise from error logs.
*   **Windows file-handle release:** Because `<video>` lives inside a cross-origin preview `<iframe>`, the React UI unmounts the preview `<iframe>` (`setPreviewSlug(null)`) via callbacks wired into both `WorkspaceRow.tsx` and `BatchActionBar.tsx` before calling `clean` or `reset --from preview`, backed by bounded retry-with-backoff in `vsplit-core`.
*   **HTTP `/api/*` additions and fixes (on top of existing workspace/batch/config/logs/open-preview routes):**
    *   `POST /api/project/open` (`{ path: string }` -> `200 { working_root, recent }` | `400` invalid path | `409` active jobs running) & `GET /api/project/recent` (`200 { working_root, recent: string[] }`).
    *   `POST /api/scan` (`{ path?: string, recursive?: boolean, relocate_policy?: "auto" | "skip" | "report", dry_run?: boolean }` -> `200 { adopted, skipped, relocated, warnings }`).
    *   `POST /api/jobs/cancel` (`{ slug?: string }` -> `200 { cancelled: number }`, cancelling matching active `CancellationToken`s while keeping `/run` endpoints synchronous with in-flight registration).
    *   `GET /api/doctor` (`200 { ffmpeg, ffprobe, encoders, gstreamer, cli_shim, path_shadowing, locks, ok }`).
    *   Fix existing `POST /api/workspaces/{slug}/unignore` response key from `{"ignored": n}` to `{"unignored": n}` (preserving count `n` for parity with `ignore`/`approve`/`clean`).

### 5. AI-Agent CLI Contract (`vsplit`)

`vsplit` (compiled as a `CONSOLE` subsystem binary on Windows, separate from `vsplit-desktop`'s `WINDOWS` subsystem binary so stdout/stderr/exit codes pipe without console-window flashes in GUI mode) links only `vsplit-core` and exposes full automation parity:

*   **Universal `--json`:** Every subcommand (`scan`, `status`, `run`, `approve`, `reset`, `clean`, `collect`, `one`, `ignore`, `unignore`, `cuts`, `logs`, `doctor`) emits valid single-object JSON to `stdout` under `--json` (`{ "ok": true, ... }` on success; `{ "ok": false, "code": "<ERR_CODE>", "message": "...", "details": {...} }` on non-zero exits), while human diagnostics go only to `stderr`.
*   **Token-efficient `status --json`:** Defaults to a compact summary projection (~100 bytes/workspace: `slug`, `stages`, `scenes_found`, `ignored`, `locked_by`, `error_summary`) so 100-clip agent polls stay under 3,000 tokens; full `manifest.json` payloads remain available via `--full`.
*   **Unified `--filter` parser:** One shared filter implementation across `run`, `approve`, `reset`, `clean`, `ignore`, and `unignore` that evaluates `slug=<glob>`, `status=<pending|running|done|failed|awaiting_user|skipped>`, and `stage=<probe|detect|preview|review|plan|split|verify>` (AND-combined when repeated) and rejects unknown keys/values with exit code `2` (fixing the match-all bug in `reset`/`clean`).
*   **Non-interactive fail-fast guard:** When `stdin` is not a TTY (or `--non-interactive` is set), commands never block on confirmation prompts; destructive or ambiguous actions fail fast with exit code `2` and a JSON remediation hint unless `--force` or `--auto-relocate`/`--no-relocate` is passed.
*   **New agent subcommands:** `vsplit ignore/unignore`, `vsplit cuts get <slug>` / `vsplit cuts set <slug> --file -|<path> [--expected-hash <sha256>]` (full schema + scene-ID validation + atomic write under `<slug>.lock`), `vsplit logs <slug> <stage> [--tail N]`, and `vsplit doctor`.
*   **Deterministic exit codes:** `0` success, `1` runtime/stage failure (fixing `vsplit run` exiting `0` on failure), `2` usage/validation/non-interactive error, `3` workspace locked by another process, `4` missing external dependency (`ffmpeg`/`ffprobe`), `10` stopped at `awaiting_user`, `130` interrupted (`SIGINT`).

### 6. Packaging, One-Click GitHub Updates & Extracted CLI Shim

Releases are built by GitHub Actions on a Windows `x86_64` + Linux `x86_64` matrix and published with Ed25519 signatures (`TAURI_SIGNING_PRIVATE_KEY`) and a static `https://github.com/forkadam/auto-video-splitter/releases/latest/download/latest.json` manifest (avoiding the 60 req/hr unauthenticated GitHub REST API limit).

*   **Windows (`x86_64` NSIS installer + `tauri-plugin-updater`):**
    *   Per-user NSIS install into `%LOCALAPPDATA%\vsplit\` packaging `vsplit-desktop.exe` (Windows GUI subsystem) alongside `vsplit.exe` (Console subsystem, bundled via Tauri `externalBin`), with `windows-hooks.nsh` adding `%LOCALAPPDATA%\vsplit\` to user `PATH` (and removing it on uninstall, broadcasting `WM_SETTINGCHANGE`).
    *   One-click **Update & Restart** blocks if the desktop app's `JobCoordinator` has active runs, verifies the Ed25519 signature, and runs the silent NSIS update (updating both binaries together; if an external `vsplit.exe` process holds the file open with `ERROR_SHARING_VIOLATION`, NSIS exits with a retryable status and the UI displays `"A vsplit CLI process is currently running; wait for it to finish and retry"`).
*   **Linux / Arch Omarchy (`x86_64` Tauri v2 AppImage + `tauri-plugin-updater` + Extracted CLI Shim):**
    *   The Tauri v2 AppImage bundles the `vsplit` CLI binary as a Tauri `resources` file alongside `vsplit-desktop`.
    *   On first launch and immediately after every `tauri-plugin-updater` upgrade (keyed by embedded version stamp), `vsplit-desktop` atomically extracts the co-versioned `vsplit` CLI binary to `~/.local/share/vsplit/bin/vsplit-<version>`, swaps the symlink `~/.local/bin/vsplit` -> `~/.local/share/vsplit/bin/vsplit-<version>`, and prunes prior versions. This keeps GUI and CLI version-locked while giving AI agents direct `< 50 ms` native CLI execution with zero AppImage FUSE mount overhead per command.
    *   `vsplit doctor` checks `fuse2` / AppImage execution health, `~/.local/bin` on `PATH`, legacy Python `vsplit` shadowing earlier in `PATH`, GStreamer H.264 plugins (`gst-inspect-1.0 avdec_h264`), and `WEBKIT_DISABLE_DMABUF_RENDERER` status.
    *   An explicit Wayland compatibility toggle in `app.toml` lets the user persist `WEBKIT_DISABLE_DMABUF_RENDERER=1` if Hyprland/GPU drivers produce blank WebKitGTK frames.

### 7. Mandatory Milestone 0 Feasibility Spike (2-Day Gate Before Full Implementation)

Before porting all 7 stages and 140+ tests, two hard gates must pass on target hardware:

1.  **Gate A — Rust FFmpeg-Pipe Four-Detector Golden Parity:** Run a prototype of the FFmpeg PPM pipe + presentation-order PTS alignment + all four Rust scorers (`content`, `adaptive`, `histogram`, `hash`) against `tests/fixtures/` and at least one real 10-bit HEVC VFR iPhone `.MOV` clip. Pass criteria: detected scene boundaries match PySceneDetect within +/-2 frames (+/-0.08 s) on canonical thresholds, decode throughput equals or exceeds Python/OpenCV on the same clip, and peak detector RSS stays `< 150 MB` independent of clip duration.
2.  **Gate B — Tauri v2 AppImage on Arch/Omarchy + Hyprland Wayland:** Build a minimal Tauri v2 AppImage embedding the `axum` loopback server and test it on an Arch/Omarchy (Hyprland Wayland) machine and Windows. Pass criteria: full UI renders at native HiDPI/fractional scale with zero blank frames or compositor crashes (recording whether `WEBKIT_DISABLE_DMABUF_RENDERER=1` is needed), HTTP 206 seeks across start/midpoint/end of H.264 `source-proxy.mp4` complete with zero `<video>` `MediaError` events (recording exact `pacman` GStreamer packages required), `tauri-plugin-updater` completes a signed test update, and extracted `~/.local/bin/vsplit --version` executes in `< 50 ms`. If Gate B exposes unfixable AppImage Wayland/Mesa breakage on Arch, switch Linux distribution to the user-space native archive fallback (with custom Ed25519 self-update) before Milestone 3 packaging.

## Alternatives Considered

### Alternative A: Tauri v2 + Bundled Python Sidecar (PyInstaller / Nuitka)

*   **Pros**: Reuses the existing Python backend, `PySceneDetect`/`OpenCV`, and 140+ pytest tests without rewriting pipeline stages or calibrating scene detectors.
*   **Cons**: Packaging `scenedetect[opencv]` (OpenCV + NumPy) with FastAPI and Uvicorn produces an estimated 150–300 MB bundle and update payload per platform, 1.5–4 s cold starts per AI-agent CLI invocation, estimated 150–250 MB resident backend RAM before WebView overhead, Windows Defender scan delays on PyInstaller's per-launch `_MEI` temp extraction directory, Arch rolling `glibc`/OpenCV ABI breakage, and three-runtime maintenance (Rust + frozen Python + React).
*   **Reason for Rejection**: Fails the `< 50 ms` CLI startup, `< 50 MB` idle backend memory, and small one-click GitHub update requirements.

### Alternative B: Keep Python Core + Thin Launcher (`chromium --app=` or Thin Tauri Shell) + `uv` Updates

*   **Pros**: Lowest upfront implementation scope, zero detector parity risk, retains all four PySceneDetect detectors and Python ML/CV extensibility, and avoids AppImage/WebKitGTK packaging if using `chromium --app=`.
*   **Cons**: Retains Python/FastAPI startup latency and memory overhead (estimated 80–150 MB), depends on `uv tool upgrade` rather than standalone one-click GitHub binary updates, and `chromium --app=` lacks native OS folder dialogs and desktop shell integration.
*   **Reason for Rejection**: Trading away full-rewrite effort would leave the `< 50 ms` AI-agent CLI latency, `< 50 MB` idle backend memory, and standalone one-click GitHub binary update requirements unmet.

### Alternative C: Pure Rust with C-FFI (`opencv-rust` / `ffmpeg-next`) or Reduced Detector Scope

*   **Pros**: Linking `libav*`/`opencv` avoids frame piping; limiting Rust detection to fewer detectors reduces initial calibration scope (or delegates remaining detectors to an optional external `scenedetect` CLI).
*   **Cons**: Linking dynamic C/C++ libraries breaks on Arch rolling `ffmpeg`/`opencv` soname bumps and complicates Windows MSVC CI. Dropping `histogram` or `hash` removes config parity with v1 `.vsplit/config.yaml`, and relying on external `scenedetect` reintroduces a Python dependency on clean machines.
*   **Reason for Rejection**: Piping downscaled PPM frames from external `ffmpeg` into Rust implementations of all four live PySceneDetect algorithms avoids C-FFI build fragility while preserving full v1 config compatibility.

### Alternative D: Linux User-Space Native Archive or Arch `PKGBUILD`/AUR Channel Instead of Tauri v2 AppImage

*   **Pros**: A user-space native archive (`~/.local/share/vsplit/` + custom Rust self-updater) links directly against host Arch Mesa/Wayland and GStreamer; an Arch `PKGBUILD`/AUR package delegates `webkit2gtk-4.1`, `gst-libav`, and `ffmpeg` dependencies to `pacman`.
*   **Cons**: A custom user-space archive requires maintaining custom Rust signature-verification and symlink-swap code outside Tauri's standard updater; an AUR channel gives up one-click in-app self-updates on Linux in favor of running `yay`/`paru`.
*   **Reason for Rejection**: Standardizing on Tauri v2 AppImage with `tauri-plugin-updater` and an extracted `~/.local/bin/vsplit` CLI shim reuses Tauri's built-in updater across Windows and Linux, with Gate B of the Milestone 0 spike acting as the explicit fallback trigger to a user-space native archive if AppImage Wayland/Mesa conflicts occur on Omarchy.

### Alternative E: Tauri Native `invoke()` / `listen()` IPC for RPC + Media-Only Loopback HTTP

*   **Pros**: Removes all mutating endpoints (`clean`, `reset`, `run`, `config`, `collect`) from localhost TCP and eliminates SSE connection handling inside the desktop app.
*   **Cons**: Requires rewriting ~15–20 frontend API and SSE call sites behind a transport adapter and complicates Vite HMR browser development.
*   **Reason for Rejection**: Keeping REST, SSE, and HTTP 206 `<video>` on one token-authenticated embedded `axum` loopback server (`127.0.0.1:0`) preserves existing frontend view components (`WorkspaceRow`, `CutsDrawer`, `ConfigDrawer`, `LogDrawer`, `CollectDialog`, `FilterBar`, `BatchActionBar`) and Vite dev ergonomics on a single transport layer.

### Alternative F: Opportunistic Local IPC Coordinator (Unix Socket / Windows Named Pipe) on Top of OS File Locks

*   **Pros**: Enforces one shared concurrency pool when GUI and CLI run simultaneously and streams live intra-stage percentage progress from CLI runs straight into the open GUI.
*   **Cons**: Adds cross-platform Unix domain socket and Windows Named Pipe transport code, stale-endpoint cleanup after hard crashes, GUI/CLI IPC protocol versioning, and two CLI execution modes (`delegated` vs. `direct`) to test and maintain.
*   **Reason for Rejection**: OS file locks plus debounced `notify` watching provide one unified execution path for both GUI and headless AI-agent runs without cross-platform local socket maintenance.

--------------------------------------------------------------------------------

## Risks & Mitigations

*   **Arch/Omarchy AppImage Wayland, Mesa, and GStreamer conflicts:** Ubuntu-built AppImages can hit `libwayland-client`/Mesa crashes under Hyprland or miss host GStreamer H.264 plugins. *Mitigation:* Gate B tests the AppImage directly on Arch/Omarchy before Milestone 1; `app.toml` exposes `wayland_disable_dmabuf`; `vsplit doctor` checks `fuse2` and `gst-inspect-1.0 avdec_h264`; and Gate B failure triggers switching Linux packaging to a user-space native archive before Milestone 3.
*   **Network filesystem (SMB/NFS) locking and Linux `inotify` watch exhaustion:** OS file locks (`flock`/`LockFileEx`) and `inotify`/`ReadDirectoryChangesW` degrade on SMB/NFS mounts, and large directories can exceed `fs.inotify.max_user_watches` (`ENOSPC`). *Mitigation:* Scope `notify` non-recursively to metadata directories only (excluding `preview/images/` and `output/`), fall back automatically to bounded polling with a UI warning on `ENOSPC`, and document local-disk requirement.
*   **Full Python-to-Rust rewrite parity and detector drift:** Porting 7 stages, 4 detectors (including 16x16 2D-DCT pHash), schemas, and 140+ tests risks subtle regressions on VFR 10-bit HEVC iPhone `.MOV` files. *Mitigation:* Enforce Gate A golden parity against PySceneDetect before Milestone 1, keep Python intact until Milestone 4 passes the black-box conformance suite, and document live `plan.min_segment_length` and `verify` invariants explicitly.
*   **FFmpeg PPM-pipe deadlocks and uncaptured errors:** Blocking `stdout` reads without concurrently draining `stderr` can deadlock on verbose FFmpeg logs. *Mitigation:* Drain `stdout` and `stderr` concurrently on async Tokio tasks in `process.rs` and `pipe.rs`, persisting the last 16 KB of `stderr` on failure.
*   **`vsplit 0.3.0` rollback caveats:** `.vsplit/` v1 schemas remain compatible with `0.3.0`, but Windows `config.yaml` moves from `%LOCALAPPDATA%\vsplit\vsplit\config.yaml` to `%APPDATA%\vsplit\config.yaml` (so edits written by the Rust app are not read if rolling back to `0.3.0`), and `.vsplit/.locks/` directories are ignored by `0.3.0` `scan`/`clean` (verified in the Milestone 1 rollback test).
*   **Co-versioned update interruptions:** Updating while an external agent runs `vsplit.exe` on Windows triggers `ERROR_SHARING_VIOLATION`, and Linux CLI shim extraction happens on first post-update GUI launch. *Mitigation:* Gate updates on active desktop `JobCoordinator` runs, surface a clear retry prompt on Windows file-in-use errors, and check GUI-vs-CLI version alignment in `vsplit doctor`.

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Security & Privacy**:
    *   All processing is local-only; no video files, thumbnails, or workspace metadata leave the machine. Network access occurs only for checks against the static GitHub Releases manifest (`latest.json`) and signed update downloads.
    *   The embedded `axum` server binds only to `127.0.0.1:0` (never `0.0.0.0`), requires a 256-bit per-launch `X-Vsplit-Token` header plus Tauri `Origin` validation on `/api/*`, scopes media and SSE routes under `/m/{token}/...` (with token redaction in server logs), and enforces path-traversal guards on `slug`, thumbnail, preview, and `collect` paths.
    *   Release updates are verified with Ed25519 signatures (`tauri-plugin-updater` public key embedded in `tauri.conf.json`).
*   **Performance & Latency**:
    *   Desktop cold start to first interactive window: `< 1.0 s`.
    *   Metadata CLI commands (`status --json`, `cuts get --json`): `< 50 ms` cold start for up to 200 workspaces on local SSD.
    *   Subprocess-inclusive diagnostics (`doctor --json`): `< 500 ms`.
    *   Idle backend RSS (excluding WebView2/WebKitGTK): `< 50 MB`.
    *   `status --json` default payload for 100 workspaces: `< 3,000` LLM tokens (~100 bytes/workspace summary projection).
    *   Application diagnostics written via standard `tracing-appender` rotating log files under `%LOCALAPPDATA%\vsplit\logs` (Windows) and `$XDG_STATE_HOME/vsplit/logs` (Linux, defaulting to `~/.local/state/vsplit/logs`).
*   **Rollout & Rollback**:
    *   `.vsplit/` v1 schemas (`index.json`, `config.yaml`, `manifest.json`, `cuts.json`, `scenes.json`, `cuts-resolved.json`) stay backward-compatible so existing workspaces can roll back to Python `vsplit 0.3.0` without `extra="forbid"` crashes.
    *   Desktop-only settings live in `app.toml` outside `.vsplit/`.
    *   A language-agnostic conformance test suite validates CLI output, `.vsplit/` artifacts, and golden scene-detection fixtures before retiring the Python package.
    *   If Gate B of the Milestone 0 spike fails on Arch/Omarchy AppImage Wayland/Mesa rendering, Linux packaging switches to the user-space native archive fallback before packaging implementation begins.

--------------------------------------------------------------------------------

## Future Improvements

*   **Opportunistic Local IPC Progress Streaming**: If live per-frame progress bars inside the GUI for CLI-initiated runs become necessary later, add an optional local Unix socket / Named Pipe layer on top of the existing OS file locks without changing the `.vsplit/` on-disk contract.
*   **Native React Preview Pane**: Replace the `preview/index.html` iframe inside the desktop app with a native React video player component while keeping static HTML generation for headless CLI use.
*   **Additional CLI / Desktop Conveniences**: Add `vsplit run --ndjson` event streaming, `vsplit config get/set`, single-instance `vsplit gui [PATH]` window focusing, OS file-manager reveal / external player openers, and persisted window bounds (for floating window managers) if needed post-v1.
*   **Hardware-Accelerated Decode/Proxy**: Expose optional FFmpeg hardware acceleration (`vaapi` on Arch/Linux, `d3d11va`/`nvdec` on Windows) in `app.toml` for faster 4K HEVC proxy transcoding and frame piping.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

Execution proceeds in five sequential milestones, starting with the mandatory 2-day feasibility spike (**Milestone 0**) before retiring any Python code. Each milestone leaves the repository buildable and passing its verification checkpoint.

### Milestone Sequence

*   **Milestone 0 (2-Day Feasibility Spike):** Prototype the Rust FFmpeg-pipe four-detector engine and a minimal Tauri v2 AppImage + embedded `axum` range server; validate Gate A (VFR 10-bit HEVC parity) and Gate B (Arch/Omarchy Hyprland rendering, WebKitGTK H.264 scrubbing, `tauri-plugin-updater`, and `< 50 ms` extracted CLI shim cold-start for `--version`).
*   **Milestone 1 (`vsplit-core` + `vsplit-cli` + Conformance Suite):** Port all v1 schemas, locking, POSIX paths, `MediaToolchain`, 7 stages, 4 detectors, headless `scan`, and standalone agent CLI; pass black-box parity suite against existing fixtures.
*   **Milestone 2 (`vsplit-server` + Frontend Integration):** Build the embedded `axum` server with `notify` watching, token-scoped `/m/{token}/...` media and SSE routes, dynamic project switching, and `POST /api/jobs/cancel`; copy `src/vsplit/dashboard/src/` into `frontend/` (leaving Python untouched until M4) and wire React updates.
*   **Milestone 3 (`apps/desktop` Tauri v2 Shell):** Wire native folder/file pickers, `app.toml` CRUD IPC, drag-and-drop, AppImage CLI shim extraction, Windows NSIS `externalBin` packaging, Wayland DMABUF toggle, and `tauri-plugin-updater`.
*   **Milestone 4 (CI/CD Release Pipeline & Python Retirement):** Ship signed Windows NSIS + Linux AppImage releases via GitHub Actions, verify clean-machine upgrades, and delete retired Python source files once conformance tests and desktop QA pass.

### Target Files to Create/Modify/Delete

#### 1. `Cargo.toml` (Create — M0/M1)

*   **Type / Scope**: Rust Workspace Manifest
*   **Proposed Changes**: Define workspace members (`crates/vsplit-core`, `crates/vsplit-server`, `crates/vsplit-cli`, `apps/desktop/src-tauri`), shared workspace version (single source of truth), and pinned workspace dependencies: `tokio = "1.43"`, `tokio-util = "0.7"`, `serde = "1.0"`, `serde_json = "1.0"`, `serde_yml = "0.0.12"`, `toml = "0.8"`, `axum = "0.8"`, `tower-http = "0.6"`, `clap = "4.5"`, `notify = "8.0"`, `notify-debouncer-mini = "0.5"`, `fs2 = "0.4"`, `rand = "0.8"`, `tracing = "0.1"`, `tracing-subscriber = "0.3"`, `tracing-appender = "0.2"`, `thiserror = "2.0"`, `chrono = "0.4"`, `sha2 = "0.10"`, `globset = "0.4"`, `minijinja = "2.7"`, `directories = "6.0"`, `windows-sys = "0.59"` (Windows target), `nix = "0.29"` (Unix target), `tauri = "2"`, `tauri-plugin-dialog = "2"`, `tauri-plugin-updater = "2"`.

#### 2. `crates/vsplit-core/Cargo.toml` & `crates/vsplit-core/src/lib.rs` (Create — M0/M1)

*   **Type / Scope**: Core Rust Library Entrypoint
*   **Proposed Changes**: Declare core library crate and re-export all modules: `schemas`, `paths`, `atomic`, `locks`, `config`, `manifest`, `index`, `hash`, `keyframes`, `toolchain`, `process`, `slugify`, `filter`, `detect`, `stages`, `orchestrator`, `jobs`, `actions`, `scan`, `collect`, `watcher`, `doctor`.

#### 3. `crates/vsplit-core/src/schemas/{mod.rs,manifest.rs,index.rs,config.rs,cuts.rs,scenes.rs,plan.rs,app_config.rs}` (Create — M1)

*   **Type / Scope**: Data Schemas & Validation
*   **Proposed Changes**:
    *   Implement `serde` models schema-compatible with v1 `.vsplit/` files using `#[serde(deny_unknown_fields)]`: `Manifest`, `SourceInfo`, `MetadataBlock`, `StageBlock`, `StageError`, `ToolVersions`, `Index`, `WorkspaceEntry`, `Config`, `DetectConfig`, `PreviewConfig`, `ReviewConfig`, `PlanConfig`, `SplitConfig`, `VerifyConfig`, `ScanConfig`, `CollectConfig`, `CutsJson`, `Scene`, `ScenesJson`, and `CutsResolvedJson`.
    *   Enforce live v1 validators: exact 7 stage keys on `Manifest`, exact 6 stage keys on `Config.concurrency` (rejecting `review`, values `>= 1`), `^\d{2}:\d{2}:\d{2}\.\d{3}$` timecode format + unique scene IDs on `CutsJson`, exact `config_snapshot` shape (`detect`, `split`, `review.default_keep`), and explicit `schema_version` checks (returning a structured validation error on unsupported future versions).
    *   Define separate `AppConfig` (`app.toml`) outside `.vsplit/` with concrete types/defaults: `recent_folders: Vec<PathBuf>` (max 10 deduplicated entries, default `[]`), `last_folder: Option<PathBuf>` (default `None`), `ffmpeg_path: Option<PathBuf>` (default `None`), `ffprobe_path: Option<PathBuf>` (default `None`), and `wayland_disable_dmabuf: bool` (default `false`).

#### 4. `crates/vsplit-core/src/{paths.rs,atomic.rs,config.rs,manifest.rs,index.rs,slugify.rs,hash.rs,keyframes.rs}` (Create — M1)

*   **Type / Scope**: Core Filesystem, Config I/O & Utility Primitives
*   **Proposed Changes**:
    *   `paths.rs`: Resolve `.vsplit/` subdirectories, enforce slug path-traversal checks, canonicalize persisted relative paths to POSIX `/` on write and normalize `\` -> `/` on read, and resolve unified user-global config paths (`~/.config/vsplit/` on Linux, `%APPDATA%\vsplit\` on Windows with read-only fallback to `%LOCALAPPDATA%\vsplit\vsplit\config.yaml` when `%APPDATA%\vsplit\config.yaml` is absent; writes always target `%APPDATA%\vsplit\config.yaml`).
    *   `atomic.rs`: Atomic temp-file write + rename with bounded Windows `ErrorKind::PermissionDenied` / `ERROR_SHARING_VIOLATION` retry backoff and explicit warning log on retry exhaustion.
    *   `config.rs`, `manifest.rs`, `index.rs`: Port 4-layer YAML config resolution/deep-merge (`defaults` -> user-global -> project `.vsplit/config.yaml` -> explicit `--config`) and atomic `manifest.json`/`index.json` load/save helpers.
    *   `slugify.rs`: Match `src/vsplit/core/slugify.py` rules (`README.md` L510–L518): Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`), 64-char truncation, and case-insensitive `retired_slugs` collision suffixing up to `-99`.
    *   `hash.rs` & `keyframes.rs`: Partial/full SHA-256 content hashing (including raw-bytes `cuts.json` hashing for `expected_hash`) and `keyframes.csv` parser (stripping Windows ffprobe SEI annotations via comma split + `snap_preceding`).

#### 5. `crates/vsplit-core/src/locks.rs` (Create — M1)

*   **Type / Scope**: Cross-Process Concurrency & Lease Coordination
*   **Proposed Changes**: Hold exclusive `fs2` OS locks (`flock` / `LockFileEx`) on empty sentinel files `.vsplit/.locks/index.lock` and `.vsplit/.locks/<slug>.lock` (never unlinked while held), paired with atomic sibling `.vsplit/.locks/<slug>.lease.json` (`{pid, actor, stage, run_id, started_at, version}`, treated as active only while `try_lock_exclusive()` on `<slug>.lock` fails — avoiding Windows mandatory `ERROR_LOCK_VIOLATION` on peer reads). Enforce canonical lock order (`index.lock` before `<slug>.lock`), include an in-process workspace mutex map for `vsplit-desktop`, and expose `try_acquire_workspace_lock` (`LockHeldError` -> HTTP `409` / CLI exit `3`) and `inspect_workspace_lock`.

#### 6. `crates/vsplit-core/src/{toolchain.rs,process.rs,doctor.rs}` (Create — M0/M1)

*   **Type / Scope**: External Media Binary Discovery, Subprocess Runner & Diagnostics
*   **Proposed Changes**:
    *   `toolchain.rs`: Cached-per-session resolution of `ffmpeg` and `ffprobe` (`VSPLIT_FFMPEG_PATH`/`VSPLIT_FFPROBE_PATH` -> `app.toml` -> system `PATH`) plus `libx264` and `aac` encoder capability checks.
    *   `process.rs`: Unified async subprocess runner passing `CREATE_NO_WINDOW` (`0x08000000`) on Windows, concurrently draining `stdout` and `stderr` (capturing last 16 KB of `stderr` + `argv` + duration), and registering child PIDs in a Windows Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) or POSIX process group (`setpgid`) tied to a per-run `CancellationToken`.
    *   `doctor.rs`: Shared preflight diagnostic report covering `ffmpeg`, `ffprobe`, `libx264`/`aac` encoders, `PATH` shadowing (detecting stale Python `vsplit` earlier in `PATH`), Linux `fuse2`/GStreamer H.264 plugin checks (`gst-inspect-1.0 avdec_h264`), `~/.local/bin/vsplit` shim alignment, active workspace locks, and OS/Wayland session flags.

#### 7. `crates/vsplit-core/src/detect/{mod.rs,pipe.rs,content.rs,adaptive.rs,histogram.rs,hash.rs}` (Create — M0/M1)

*   **Type / Scope**: Rust FFmpeg-Pipe Four-Detector Engine (Rust Scorer)
*   **Proposed Changes**:
    *   `pipe.rs`: Spawn external `ffmpeg` with `-fps_mode passthrough -vf scale=w=min(iw\,320):-2:flags=fast_bilinear,format=rgb24 -f image2pipe -vcodec ppm pipe:1`, parse each PPM `P6 <W> <H> 255\n` frame header for exact post-rotation byte stride, align decoded frames with presentation-order PTS from `ffprobe -v error -select_streams v:0 -show_entries frame=best_effort_timestamp_time -of csv=p=0`, drain `stdout`/`stderr` concurrently, and honor `CancellationToken`.
    *   `content.rs`: RGB24 -> 8-bit HSV (OpenCV `H: 0..179`, `S/V: 0..255` scaling) weighted mean delta (`delta_hue=1, delta_sat=1, delta_lum=1`, averaged by 3) + `luma_only` mode.
    *   `adaptive.rs`: Rolling-window normalized ratio over `content` scores (`window_width=2`, `min_content_val=15.0`, honoring `luma_only`).
    *   `histogram.rs`: 256-bin YUV Y-channel (luma) normalized histogram compared via Pearson correlation (`correl <= 1.0 - threshold`, matching live `scenedetect.detectors.HistogramDetector`).
    *   `hash.rs`: 16x16 2D-DCT perceptual hash (pHash: 32x32 area-averaged grayscale, max-normalized, 2D DCT, top-left 16x16 block thresholded `> median`, normalized Hamming distance `/ 256.0 >= threshold`, matching live `scenedetect.detectors.HashDetector`).
    *   `mod.rs`: Unified `Detector` trait converting `min_scene_length` via `max(1, int(min_scene_length * (fps or 24)))` and returning scene boundaries plus summary stats logged to `logs/detect.log` (`frames_decoded`, `decode_fps`, `score_min/p50/p95/max`).

#### 8. `crates/vsplit-core/src/stages/{mod.rs,probe.rs,detect.rs,preview.rs,review.rs,plan.rs,split.rs,verify.rs}` & `crates/vsplit-core/templates/preview_index.html.j2` (Create — M1)

*   **Type / Scope**: Pipeline Stages & HTML Preview Template
*   **Proposed Changes**: Port all 7 stages with schema-compatible `.vsplit/` artifact layout and documented deltas:
    *   `probe.rs`: `ffprobe` metadata + keyframe extraction (`keyframes.csv`) via `process.rs`.
    *   `detect.rs`: Runs `detect::*` engine and writes `detect/scenes.json` (v1 schema).
    *   `preview.rs` & `preview_index.html.j2`: Sequential per-scene thumbnail extraction within each workspace (bounded globally by workspace-level `concurrency.preview`, `format=yuvj420p` for 10-bit HEVC), generates `source-proxy.mp4` whenever `preview.proxy_height > 0` (`format=yuv420p`), and renders `preview/index.html` via `minijinja` using adapted `preview_index.html.j2` (replacing Python `%` formatting on lines 47, 52, 55 with pre-formatted context fields `id_padded`, `duration_str`, `drift_str`).
    *   `review.rs`: Initial `cuts.json` generation, `cuts.json.template` auto-apply, `_validate_ids` parity (`README.md` L430–L473), plus new non-destructive `recover()` behavior: if `decisions/cuts.json` exists and validates, preserve it and restore `review` status to `awaiting_user` rather than deleting `decisions/`.
    *   `plan.rs`: Keyframe snapping, `merge_with_next` adjacency rules, label sanitization, and `plan/cuts-resolved.json` (preserving live `plan.py:56-65` behavior: `plan.min_segment_length` is kept for YAML compatibility and does NOT drop kept segments; only degenerate `end_req <= start_req` spans are dropped into `artifacts.dropped_degenerate`).
    *   `split.rs`: Lossless `-map 0 -map -0:d -c copy -avoid_negative_ts make_zero` per segment with mid-stage cancellation and validated `recover()` (probing partial outputs via `ffprobe` and deleting unreadable files).
    *   `verify.rs`: `ffprobe` duration check against `duration_tolerance_seconds` (preserving live `verify.py:22-49` semantics: returns `done` with `report.all_ok: false` on mismatches rather than `failed`).

#### 9. `crates/vsplit-core/src/{orchestrator.rs,jobs.rs,filter.rs,scan.rs,actions.rs,collect.rs,watcher.rs}` (Create — M1)

*   **Type / Scope**: Orchestration, Headless Actions & File Watcher
*   **Proposed Changes**:
    *   `orchestrator.rs`: Lease-aware crash recovery (recovers `running` stages only when `.vsplit/.locks/<slug>.lock` is acquired exclusively; writes `manifest.json` only when modified), per-stage `tokio::sync::Semaphore` concurrency limits, and per-stage rotating logs (`logs/<stage>.log`) with `run_id` headers.
    *   `jobs.rs`: `JobCoordinator` tracking active workspace `CancellationToken`s and emitting structured lifecycle events (`stage_started`, `stage_finished`, `stage_failed`, `run_finished`).
    *   `filter.rs`: Unified filter parser/evaluator (`slug=`, `status=`, `stage=`, AND-combined) shared by all CLI commands and API routes, rejecting unknown filter keys/values with exit code `2` / HTTP `422`.
    *   `scan.rs`: Headless folder/file scanner with `recursive` flag, `relocate_policy: Auto | Skip | Report`, long-path warning (>240 chars on Windows), and exclusive `index.lock` protection.
    *   `actions.rs` & `collect.rs`: Shared `run`, `reset`, `clean` (with Windows retry-backoff rmtree, acquiring `index.lock` before `<slug>.lock`), `approve`, `ignore`, `unignore`, `cuts_get`/`cuts_set` (holding `<slug>.lock` + `expected_hash` SHA-256 check), `config_get`/`config_set`, `logs_read`, and `collect_outputs`.
    *   `watcher.rs`: Debounced (~100 ms) `notify` watcher scoped non-recursively to `.vsplit/`, `.vsplit/.locks/`, and each workspace's root and `decisions/` directory (excluding `preview/images/` and `output/`, with polling fallback on `ENOSPC`), emitting change events and explicit watcher-error events.

#### 10. `crates/vsplit-server/Cargo.toml` & `crates/vsplit-server/src/{lib.rs,auth.rs,session.rs,sse.rs,routes/{mod.rs,project.rs,workspaces.rs,actions.rs,config.rs,logs.rs,media.rs,doctor.rs}}` (Create — M0/M2)

*   **Type / Scope**: Embedded Loopback HTTP / SSE / Media Server (Linked exclusively by `vsplit-desktop`)
*   **Proposed Changes**:
    *   `lib.rs` & `auth.rs`: Bind `axum` to `127.0.0.1:0`, generate 256-bit `session_token` (or honor `VSPLIT_DEV_TOKEN` in dev builds), enforce `X-Vsplit-Token` + Tauri `Origin` validation on `/api/*`, and expose path-scoped routes `/m/{token}/workspaces/{slug}/preview/{*path}`, `/m/{token}/workspaces/{slug}/thumbs/{name}`, and `/m/{token}/events` (redacting `{token}` from logs) so relative links inside `preview/index.html` and `EventSource` authenticate without cross-origin cookies.
    *   `session.rs`: Manage mutable `ActiveProject` (`working_root`, active `notify` watcher, `JobCoordinator`), rejecting `POST /api/project/open` with HTTP `409` if jobs are active unless cancelled first.
    *   `sse.rs`: Broadcast channel emitting `sync_lagged` on subscriber overflow plus `workspace_updated`, `workspaces_added`, `workspaces_removed`, `cuts_changed` (`{ slug, hash }`), `config_changed`, `lock_changed` (`{ slug, locked_by }`), `run_started`, `run_finished`, and `heartbeat` at `/m/{token}/events`.
    *   `routes/*.rs`: Port existing REST endpoints (omitting unused `/api/config/schema`, keeping `POST /api/workspaces/{slug}/open-preview`), fix `unignore` to return `{"unignored": n}`, add `POST /api/project/open`, `GET /api/project/recent`, `POST /api/scan`, `POST /api/jobs/cancel`, `GET /api/doctor`, and HTTP `206 Partial Content` range serving via `tower_http::services::ServeFile` suppressing expected `BrokenPipe`/`ConnectionReset` seek-abort errors from error logs.

#### 11. `crates/vsplit-cli/Cargo.toml` & `crates/vsplit-cli/src/{main.rs,output.rs,exit_codes.rs,commands/{mod.rs,scan.rs,run.rs,status.rs,approve.rs,reset.rs,clean.rs,collect.rs,one.rs,ignore.rs,unignore.rs,cuts.rs,logs.rs,doctor.rs}}` (Create — M1)

*   **Type / Scope**: Standalone AI-Agent CLI (`vsplit`, `CONSOLE` subsystem on Windows; depends only on `vsplit-core`)
*   **Proposed Changes**:
    *   Implement CLI subcommands via `clap` with global `--working-dir`, `--config`, `--json`, `--non-interactive`, and `--log-level`.
    *   Explicit subcommand flags: `scan [PATH] [--recursive] [--auto-relocate|--no-relocate] [--dry-run]`; `status [SLUG] [--awaiting-user] [--failed] [--full]`; `run [SLUG_GLOB] [--only|--from|--through] [--filter K=V] [--concurrency S=N] [--dry-run]`; `approve`/`ignore`/`unignore` `[SLUG_GLOB] [--filter K=V]`; `reset [SLUG_GLOB] (--from STAGE | --all) [--filter K=V] [--force]`; `clean [SLUG_GLOB] [--all] [--filter K=V] [--force]`; `collect [SLUG_GLOB] [--dest PATH] [--mode copy|move|link] [--force] [--dry-run]`; `one FILE_PATH [--through STAGE] [--auto-approve]`; `cuts get <slug>` & `cuts set <slug> --file -|<path> [--expected-hash <sha256>]`; `logs <slug> <stage> [--tail N]`; `doctor`.
    *   JSON output contract: `--json` emits `{ "ok": true, ... }` on success (compact `status`: `slug`, `stages`, `scenes_found`, `ignored`, `locked_by`, `error_summary`) and `{ "ok": false, "code": "...", "message": "...", "details": {...} }` on non-zero exits (`1`, `2`, `3`, `4`, `10`, `130`). Non-TTY stdin fails fast with exit code `2` instead of blocking on prompts.

#### 12. `apps/desktop/{package.json,src-tauri/{Cargo.toml,tauri.conf.json,capabilities/default.json,build.rs,src/main.rs,src/lib.rs,src/commands.rs,src/cli_shim.rs,src/updater.rs,installer/windows-hooks.nsh}}` (Create — M0/M3)

*   **Type / Scope**: Tauri v2 Desktop Shell (`vsplit-desktop`, `WINDOWS` subsystem on Windows)
*   **Proposed Changes**:
    *   `apps/desktop/package.json`: Declare `@tauri-apps/cli` devDependency and `"tauri"` script so `cd apps/desktop && bun run tauri build` executes directly.
    *   `tauri.conf.json` & `capabilities/default.json`: Configure window defaults, CSP (`connect-src http://127.0.0.1:*`, `media-src http://127.0.0.1:*`, `frame-src http://127.0.0.1:*`), `tauri-plugin-dialog`, `tauri-plugin-updater` (pointing to static `https://github.com/forkadam/auto-video-splitter/releases/latest/download/latest.json` with Ed25519 pubkey), Windows NSIS bundle co-packaging `vsplit.exe` via `externalBin` + `windows-hooks.nsh` (adding/removing `%LOCALAPPDATA%\vsplit\` in user `PATH` and broadcasting `WM_SETTINGCHANGE`), and Linux AppImage bundle co-packaging `vsplit` via `resources`.
    *   `src/main.rs` & `src/lib.rs`: Configure `tracing-appender` rotating app logs under `%LOCALAPPDATA%\vsplit\logs` (Windows) and `$XDG_STATE_HOME/vsplit/logs` (Linux), apply persisted `wayland_disable_dmabuf` (`WEBKIT_DISABLE_DMABUF_RENDERER=1`) before GTK init, boot embedded `vsplit-server` on `127.0.0.1:0`, trigger `cli_shim::sync_if_needed()` on Linux AppImage startup, and signal `CancellationToken` on active `JobCoordinator` tasks with bounded child-process teardown on window close.
    *   `src/commands.rs`: Expose Tauri IPC commands `get_server_bootstrap` (`{ baseUrl, token }`), `get_app_config` / `set_app_config` (`AppConfig` CRUD), `pick_folder`, `pick_file`, `check_and_install_update`, and `repair_cli_shim`.
    *   `src/cli_shim.rs`: On Linux AppImage boot/post-update, atomically extract co-versioned `vsplit` CLI into `~/.local/share/vsplit/bin/vsplit-<version>`, swap `~/.local/bin/vsplit` symlink, and prune older versions.
    *   `src/updater.rs`: Block updates while desktop `JobCoordinator` has active runs; on Windows NSIS `ERROR_SHARING_VIOLATION`, display a plain retry message instructing the user to wait for active CLI commands to finish.

#### 13. `frontend/` (Copy & Modify from `src/vsplit/dashboard/src/` in M2; delete original Python copy in M4)

*   **Type / Scope**: React 19 + TypeScript + Vite 8 + Tailwind v4 + shadcn Frontend
*   **Proposed Changes**: Copy `src/vsplit/dashboard/src/` to `frontend/` in Milestone 2 (keeping `src/vsplit/dashboard/` intact until Milestone 4), then create/modify/delete:
    *   `frontend/package.json` (Modify): Add `@tauri-apps/api`, `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-updater`; remove `"gen-types"` script.
    *   `frontend/scripts/gen-types.ts` (Delete): Currently spawns `uv run vsplit` against FastAPI `/openapi.json`; removed as part of Rust migration.
    *   `frontend/vite.config.ts` (Modify): Set `build.outDir` to `dist/` consumed by `apps/desktop/src-tauri/tauri.conf.json`.
    *   `frontend/src/lib/bootstrap.ts` (Create): Fetch `{ baseUrl, token }` via Tauri `invoke("get_server_bootstrap")` (falling back to `VITE_VSPLIT_DEV_URL` / `VITE_VSPLIT_DEV_TOKEN` only during local Vite HMR development).
    *   `frontend/src/lib/api.ts` (Modify): Inject `X-Vsplit-Token` on all `fetch` calls (including `api.workspaces.log` at L67–L68, replacing bare `fetch`), remove dead `api.config.schema`, route `thumbUrl` and new `previewUrl` through `/m/{token}/...`, fix `unignore` return type to `{ unignored: number }`, add `project.open`/`recent`, `scan`, `jobs.cancel`, `doctor`, and `locked_by` field on `Workspace`.
    *   `frontend/src/lib/sse.ts` (Modify): Connect `EventSource` to `${baseUrl}/m/${token}/events`, handle `cuts_changed`, `config_changed`, `lock_changed`, and trigger full refetch on `sync_lagged`.
    *   `frontend/src/App.tsx` (Modify): Replace hardcoded iframe URL (L239) with `api.workspaces.previewUrl(previewSlug)`, keep existing `api.workspaces.openPreview` for the "Open externally" button, add header folder switcher + recent-projects dropdown, drag-and-drop overlay for folders and single videos, in-app **Scan** button in header and empty state (replacing the CLI-only empty-state prompt), active-job **Cancel** button (`POST /api/jobs/cancel`), **Doctor** diagnostics button, one-click **Update & Restart** pill, and pass an `onBeforeMutateWorkspace` callback (`setPreviewSlug(null)`) to unmount the cross-origin preview `<iframe>` before `clean`/`reset`.
    *   `frontend/src/views/WorkspaceRow.tsx` & `frontend/src/views/BatchActionBar.tsx` (Modify): Render live external-CLI lock badge (`Running <stage> via CLI`), disable destructive `reset`/`clean` controls on locked rows, and invoke `onBeforeMutateWorkspace` before `clean`/`reset`.
    *   `frontend/src/views/CutsDrawer.tsx` (Modify): Listen for `cuts_changed` SSE events; when unsaved edits exist, display a non-destructive "External edit detected — Reload / Keep Mine" conflict banner rather than overwriting unsaved edits.
    *   `frontend/src/views/ConfigDrawer.tsx` (Modify): Reload untouched fields on `config_changed` SSE events (prompting if textarea has unsaved edits) and add desktop settings tab backed by Tauri `get_app_config`/`set_app_config`/`pick_file` (`ffmpeg_path`, `ffprobe_path`, `wayland_disable_dmabuf`, CLI shim status).
    *   `frontend/src/views/DoctorDialog.tsx` (Create): Render `/api/doctor` diagnostics (`ffmpeg`/`ffprobe`/`libx264`/`aac`, Linux GStreamer codecs, CLI `PATH`/shadowing status, active locks) with copyable remediation commands and `pick_file` binary pickers.

#### 14. `crates/vsplit-core/tests/`, `crates/vsplit-cli/tests/`, `crates/vsplit-server/tests/`, & `tests/fixtures/` (Create — M0/M1/M2/M3)

*   **Type / Scope**: Conformance Suite, Detector Golden Fixtures & Unit/Integration Tests
*   **Proposed Changes**:
    *   `tests/fixtures/golden/{short_mp4,short_mkv}_*.json` & synthetic VFR 10-bit HEVC generator in `tests/fixtures/make_fixtures.py` (M0): Recorded PySceneDetect golden JSON snapshots for `content`, `adaptive`, `histogram`, and `hash` (plus `#[ignore]`-gated local real-iPhone `.MOV` test).
    *   `crates/vsplit-core/tests/golden_detect.rs` (M0): Runs via `cargo test -p vsplit-core --test golden_detect -- --nocapture`, asserting all four detectors match golden snapshots within +/-2 frames (+/-0.08 s).
    *   `crates/vsplit-core/tests/{schemas_v1.rs,slugify_paths.rs,locks_recovery.rs,stages_pipeline.rs,scan_collect.rs}` (M1): Port 140+ unit edge cases (Windows `CON`/`PRN`/`AUX`/`NUL`/`COM1..9`/`LPT1..9`, atomic replace retries, `cuts.json` ID/timecode validation, `merge_with_next` gap guard, `plan.min_segment_length` no-op, `verify` `done`-with-`all_ok: false`, new non-destructive `review.rs` recovery, `#[serde(deny_unknown_fields)]` v1 compatibility, read-only legacy `platformdirs` fallback, `scan` `Auto`/`Skip`/`Report` relocation + Windows >240-char warning, and mid-stage `ffmpeg` cancellation killing child processes).
    *   `crates/vsplit-cli/tests/cli_contract.rs` (M1): Black-box CLI tests asserting `--json`, exit codes (`0`, `1`, `2`, `3`, `4`, `10`, `130`), non-interactive TTY fail-fast, unified `--filter` (including `reset`/`clean` `stage=` regression test), compact `status --json` token size, POSIX relative paths, and two-process lock contention.
    *   `crates/vsplit-server/tests/{auth_media.rs,watcher_sse.rs}` (M2): Integration tests for `X-Vsplit-Token` + `Origin` rejection (`401`/`403`), `/m/{token}/...` HTTP 206 Range serving, `notify` watcher refresh on external `cuts.json`/`config.yaml` edits, `sync_lagged` recovery, and `POST /api/jobs/cancel`.
    *   `apps/desktop/src-tauri/src/cli_shim.rs` unit tests (M3): Verify atomic extraction, symlink swap, and old-version pruning in temp dirs.

#### 15. `.github/workflows/ci.yml` & `.github/workflows/release.yml` (Create — M4)

*   **Type / Scope**: CI & Signed Cross-Platform Release Workflows
*   **Proposed Changes**:
    *   `ci.yml`: Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (with `ffmpeg` installed), and `cd frontend && bun run build && bun run lint && bunx tsc -b` on `windows-latest` and `ubuntu-latest`.
    *   `release.yml`: Trigger on `v*` git tags; build co-versioned `vsplit` + `vsplit-desktop` on Windows `x86_64` (NSIS) and Linux `x86_64` (Tauri v2 AppImage bundling `vsplit`), sign artifacts with `TAURI_SIGNING_PRIVATE_KEY`, and upload signed binaries plus static `latest.json` to GitHub Releases.

#### 16. Retired Python Package, `.gitignore`, `.gitattributes`, & `README.md` (Create / Modify / Delete — M4, gated on conformance suite pass)

*   **Type / Scope**: Retirement Cleanup & Documentation
*   **Proposed Changes**:
    *   Delete `src/vsplit/`, `pyproject.toml`, `tests/conftest.py`, `tests/unit/`, and `tests/integration/` once the Rust conformance suite passes and Milestone 3 desktop QA succeeds on Windows and Arch/Omarchy (`uv.lock` is already gitignored).
    *   Modify `.gitignore` to ignore `target/`, `frontend/dist/`, and `frontend/node_modules/`.
    *   Create `.gitattributes` locking LF line endings for source/config files.
    *   Modify `README.md` to document Windows NSIS and Arch/Omarchy AppImage installation, `~/.local/bin/vsplit` CLI shim, AI-agent `--json`/exit-code contract, `vsplit doctor`, and one-click GitHub updates.

### Verification Plan

*   **Milestone 0 Spike Checkpoint (Hard Go/No-Go Gate):**
    *   `cargo test -p vsplit-core --test golden_detect -- --nocapture` against `tests/fixtures/` + a 10-bit HEVC VFR iPhone `.MOV` clip: verify all four detectors match PySceneDetect boundaries within +/-2 frames (+/-0.08 s), decode throughput equals or exceeds Python/OpenCV, and peak detector RSS stays `< 150 MB`.
    *   Launch the prototype Tauri v2 AppImage on an Arch/Omarchy (Hyprland Wayland) machine and Windows: verify zero blank frames or DMABUF compositing artifacts at native HiDPI/fractional scaling, HTTP 206 start/mid/end seeks of H.264 `source-proxy.mp4` with zero `<video>` `MediaError` events, a signed test self-update via `tauri-plugin-updater`, and `< 50 ms` execution of extracted `~/.local/bin/vsplit --version`.
*   **Milestone 1 Checkpoint:** `cargo test -p vsplit-core -p vsplit-cli` passes all unit, `cli_contract`, lock-contention, and `vsplit 0.3.0` rollback tests (opening a Rust-modified workspace under archived `0.3.0` Pydantic validation with zero `extra="forbid"` errors).
*   **Milestone 2 Checkpoint:** `cargo test -p vsplit-server` and `cd frontend && bun run build && bun run lint && bunx tsc -b` pass (including `CutsDrawer` 409 conflict banner and preview `<iframe>` unmount verification).
*   **Milestone 3 Checkpoint:** `cd apps/desktop && bun run tauri build` produces working Windows NSIS and Linux AppImage artifacts and passes `cli_shim` extraction/pruning tests.
*   **Milestone 4 Final Compilation / Build:**
    *   `cargo build --workspace --release`
    *   `cd frontend && bun run build`
    *   `cd apps/desktop && bun run tauri build`
*   **Unit & Conformance Tests:**
    *   `cargo test --workspace`
    *   Concurrency lock-contention test: spawn two overlapping `vsplit run` processes on the same workspace and verify the second exits with code `3` (`WORKSPACE_LOCKED`) without corrupting `manifest.json` or deleting in-flight outputs.
*   **Static Analysis & Linting:**
    *   `cargo fmt --all -- --check`
    *   `cargo clippy --workspace --all-targets -- -D warnings`
    *   `cd frontend && bun run lint && bunx tsc -b`
*   **Test Coverage Requirement:**
    *   `>= 85%` line coverage on `crates/vsplit-core` and `crates/vsplit-cli` on stable Rust (measured via `cargo llvm-cov -p vsplit-core -p vsplit-cli`), with `100%` line coverage on `crates/vsplit-core/src/locks.rs`, `crates/vsplit-core/src/filter.rs`, `crates/vsplit-core/src/schemas/cuts.rs`, and `crates/vsplit-cli/src/exit_codes.rs`.
*   **Performance & Cross-OS E2E Benchmarks:**
    *   Measure desktop startup (`< 1000 ms`), idle backend RSS (`< 50 MB`), `vsplit status --json` cold start (`< 50 ms` up to 200 workspaces) and payload size (`< 3,000` tokens for 100 workspaces), and `vsplit doctor --json` (`< 500 ms`) on both Windows and Arch/Omarchy.
    *   Run end-to-end desktop + AI-agent CLI smoke test on Windows and Arch/Omarchy verifying CUJs 1–4.

--------------------------------------------------------------------------------

*Drafted with EGM*
