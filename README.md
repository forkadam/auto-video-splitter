# vsplit

Local, scene-aware video splitter with a native desktop app (**Windows** & **Linux / Arch Omarchy Hyprland**) and an **AI-agent-ready CLI**.

`vsplit` scans folders of video clips, detects scene boundaries in pure Rust over an FFmpeg PPM pipe (`content`, `adaptive`, `histogram`, `hash`), renders thumbnail + H.264 proxy previews, pauses at a human review gate (`decisions/cuts.json`), and losslessly stream-copies kept scenes via FFmpeg (`-c copy`). Desktop and CLI operate directly on the same `.vsplit/` directory with OS file locks and live two-way sync.

---

## Install

### Prerequisites
External `ffmpeg` and `ffprobe` (with `libx264` and `aac` encoders):
- **Windows:** `winget install Gyan.FFmpeg`
- **Arch Linux / Omarchy:** `sudo pacman -S ffmpeg gst-libav gst-plugins-good gst-plugins-bad fuse2`

Run `vsplit doctor` (or open **Doctor** in the desktop app) to verify FFmpeg, encoders, Linux GStreamer H.264 playback, `PATH` shadowing, and active workspace locks. Custom binary paths can also be configured in machine-local `app.toml`.

### Prebuilt Releases (GUI + CLI in Lockstep)
Download from [GitHub Releases](https://github.com/forkadam/auto-video-splitter/releases):
- **Windows (`x86_64`):** Run `vsplit_<version>_x64-setup.exe` (per-user NSIS install into `%LOCALAPPDATA%\vsplit\`, bundling both `vsplit-desktop.exe` and console `vsplit.exe` and adding `%LOCALAPPDATA%\vsplit\` to user `PATH`).
- **Linux / Arch Omarchy (`x86_64`):** Make `vsplit_<version>_amd64.AppImage` executable and launch it. On first launch (and after every one-click update), `vsplit-desktop` atomically extracts the co-versioned CLI to `~/.local/share/vsplit/bin/vsplit-<version>` and links `~/.local/bin/vsplit` so CLI commands run natively in `< 20 ms` with zero AppImage FUSE startup cost.

One-click updates are delivered via signed GitHub Releases (`tauri-plugin-updater` checking `releases/latest/download/latest.json`).

### Build From Source
Prerequisites: Rust stable toolchain, [Bun](https://bun.sh/), FFmpeg/FFprobe (and `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` on Linux).

```bash
# CLI only
cargo build -p vsplit-cli --release

# Full desktop app + installer (builds frontend, stages CLI sidecar, bundles NSIS/AppImage)
cd apps/desktop
bun install
bun run build
```

---

## Quick Start

### Desktop App
Launch `vsplit`, pick or drag-and-drop a video folder (or drop a single clip to adopt it directly), click **Scan**, run `probe → detect → preview → review`, scrub the inline preview, adjust scenes in the Cuts Drawer, and click **Save & continue**.

### CLI & AI-Agent Automation
Every CLI subcommand supports `--json`, deterministic exit codes, unified `--filter`, and non-interactive fail-fast behavior when `stdin` is not a TTY:

```bash
# 1. Verify toolchain
vsplit --json doctor

# 2. Scan folder and inspect compact status (<3,000 tokens for 100 clips)
vsplit -w ./clips --json scan --auto-relocate
vsplit -w ./clips --json status

# 3. Run through the review gate (exits 10 when workspaces pause at awaiting_user)
vsplit -w ./clips --json run --through review

# 4. Inspect and atomically update cuts.json with optimistic SHA-256 locking
vsplit -w ./clips --json cuts get my-clip
vsplit -w ./clips --json cuts set my-clip --file edited_cuts.json --expected-hash <sha256>

# 5. Approve (if cuts.json unchanged), finish lossless split + verify, and collect outputs
vsplit -w ./clips --json approve "*"
vsplit -w ./clips --json run
vsplit -w ./clips --json collect --dest ./collected --mode copy
```

---

## Global Options & Exit Codes

```text
vsplit [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

| Global Option | Default | Purpose |
|---|---|---|
| `-w, --working-dir PATH` | `.` | Folder containing source clips and `.vsplit/` |
| `--config PATH` | none | Explicit pipeline `config.yaml` override (never overrides `app.toml`) |
| `--json` | off | Emit single-object JSON (`{"ok": true, ...}` / `{"ok": false, "code": ..., "message": ..., "details": ...}`) on stdout |
| `--non-interactive` | off | Never prompt on stdin; fail fast with exit code `2` (also automatic when stdin is not a TTY) |
| `--log-level` | `warn` | Stderr log verbosity (`debug`, `info`, `warn`, `error`) |

| Exit Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Runtime or pipeline stage failure |
| `2` | Usage, schema validation, filter, hash mismatch, or non-interactive confirmation error |
| `3` | Workspace locked by another running `vsplit` or `vsplit-desktop` process (`WORKSPACE_LOCKED`) |
| `4` | Missing external dependency (`ffmpeg` / `ffprobe` / encoders) |
| `10` | One or more workspaces paused at `review` (`awaiting_user`) |
| `130` | Interrupted by signal (`SIGINT`) |

---

## CLI Commands

| Command | Description |
|---|---|
| `vsplit scan [PATH]` | Adopt videos (`--recursive/--no-recursive`, `--auto-relocate/--no-relocate`, `--dry-run`) |
| `vsplit status [SLUG]` | Compact pipeline status (`--awaiting-user`, `--failed`, `--full`) |
| `vsplit run [SLUG_GLOB]` | Advance stages (`--only`, `--from`, `--through`, `--filter K=V`, `--concurrency S=N`, `--dry-run`) |
| `vsplit approve [SLUG_GLOB]` | Approve `awaiting_user` workspaces (`--filter K=V`) |
| `vsplit ignore / unignore [SLUG_GLOB]` | Park or unpark workspaces (`--filter K=V`) |
| `vsplit reset [SLUG_GLOB]` | Reset stages (`--from STAGE` or `--all`, `--filter K=V`, `--force`) |
| `vsplit clean [SLUG_GLOB]` | Delete workspace state without touching source videos (`--all`, `--filter K=V`, `--force`) |
| `vsplit collect [SLUG_GLOB...]` | Gather `split=done` outputs (`--dest/--to`, `--mode copy\|move\|link`, `--force`, `--dry-run`) |
| `vsplit one FILE_PATH` | Adopt & process a single clip without scanning siblings (`--through`, `--auto-approve`) |
| `vsplit cuts get <SLUG>` / `set <SLUG>` | Read or validate+write `decisions/cuts.json` (`--file -\|PATH`, `--expected-hash <sha256>`) |
| `vsplit logs <SLUG> <STAGE>` | Read per-stage logs (`--tail N`) |
| `vsplit doctor` | Verify FFmpeg, FFprobe, encoders, Linux GStreamer/fuse2, CLI shim, PATH shadowing, and locks |

Unified `--filter` predicates (AND-combined when repeated): `slug=<glob>`, `status=<pending|running|done|failed|awaiting_user|skipped>`, `stage=<probe|detect|preview|review|plan|split|verify>`.

---

## The 7 Pipeline Stages

| # | Stage | Writes | Notes |
|---|---|---|---|
| 1 | `probe` | `probe/ffprobe.json`, `probe/keyframes.csv` | Stream metadata + keyframe PTS list |
| 2 | `detect` | `detect/scenes.json` | Pure-Rust FFmpeg PPM-pipe detector (`content`, `adaptive`, `histogram`, `hash`) |
| 3 | `preview` | `preview/index.html`, `preview/images/*.jpg`, `preview/source-proxy.mp4` | Thumbnails (`yuvj420p`) + playable H.264 proxy (`yuv420p`) |
| 4 | `review` | `decisions/cuts.json` | Human/agent gate (`awaiting_user` until edited or approved; supports `cuts.json.template`) |
| 5 | `plan` | `plan/cuts-resolved.json` | Keyframe snapping (`nearest_preceding`), adjacent `merge_with_next`, label sanitization |
| 6 | `split` | `output/*.mp4` | Lossless `ffmpeg -map 0 -map -0:d -c copy -avoid_negative_ts make_zero` |
| 7 | `verify` | `manifest.stages.verify.artifacts.report` | FFprobe output duration check against `duration_tolerance_seconds` |

---

## Configuration & Cross-OS Files

- **Pipeline config (`config.yaml`):** 4-layer deep merge: built-in defaults → user-global (`~/.config/vsplit/config.yaml` on Linux; `%APPDATA%\vsplit\config.yaml` on Windows, with read-only fallback to legacy `%LOCALAPPDATA%\vsplit\vsplit\config.yaml`) → project `.vsplit/config.yaml` → explicit `--config`.
- **Desktop settings (`app.toml`, outside `.vsplit/`):** `~/.config/vsplit/app.toml` (Linux) / `%APPDATA%\vsplit\app.toml` (Windows) storing `recent_folders`, `last_folder`, `ffmpeg_path`, `ffprobe_path`, and `wayland_disable_dmabuf`.
- **Two-way sync & locking:** Cooperating GUI and CLI processes synchronize via exclusive OS locks (`flock` / `LockFileEx`) on `.vsplit/.locks/index.lock` and `.vsplit/.locks/<slug>.lock` paired with readable `.vsplit/.locks/<slug>.lease.json` sidecars and a non-recursive debounced `notify` watcher.
- **Wayland Hyprland note:** If WebKitGTK renders a blank pane on your GPU driver, enable **"Disable WebKitGTK DMABUF renderer on Wayland"** in Configuration (persisted to `app.toml`) or launch with `VSPLIT_DISABLE_DMABUF=1`.

---

## Workspace Layout & Development

- `crates/vsplit-core` — Shared Rust pipeline, v1 schemas, locking, toolchain, 4 detectors, 7 stages, watcher
- `crates/vsplit-cli` — Standalone native `vsplit` CLI binary (links only `vsplit-core`)
- `crates/vsplit-server` — Embedded `axum` loopback HTTP/SSE/HTTP 206 media server (linked only by `vsplit-desktop`)
- `apps/desktop` — Tauri v2 desktop shell, NSIS hooks, Linux AppImage CLI shim, and updater
- `frontend` — React 19 + TypeScript + Vite 8 + Tailwind v4 + shadcn/ui interface

Run full checks locally:
```bash
python3 tests/fixtures/make_fixtures.py
cd frontend && bun install && bun run build && bun run lint && bunx tsc -b && cd ..
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Release Signing Setup
1. Generate an Ed25519 signing keypair once (keep the private `.key` file outside the repo and never commit it):
   - **Windows (PowerShell):**
     ```powershell
     cd apps/desktop
     bun run tauri signer generate -w "$env:USERPROFILE\.tauri\vsplit.key"
     ```
   - **Linux / macOS:**
     ```bash
     cd apps/desktop
     bun run tauri signer generate -w "$HOME/.tauri/vsplit.key"
     ```
2. Copy the contents of `vsplit.key.pub` into `apps/desktop/src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
3. Add **Repository secrets** in GitHub (**Settings → Secrets and variables → Actions → Secrets tab → Repository secrets → New repository secret** — *not* Environment secrets):
   - `TAURI_SIGNING_PRIVATE_KEY`: full contents of `vsplit.key` (on Windows PowerShell: `Get-Content -Raw "$env:USERPROFILE\.tauri\vsplit.key" | Set-Clipboard`).
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password you entered when generating the key (skip if you left it blank).
4. Push a version tag to build and sign a Draft GitHub Release (Windows `x64` NSIS + Linux `x64` AppImage + `latest.json`):
   ```bash
   git tag v0.4.0-alpha.0
   git push origin v0.4.0-alpha.0
   ```
