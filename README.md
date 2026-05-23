# vsplit

Local, scene-aware video splitter with a human review gate. Built for batches of personal video on Windows/macOS.

vsplit walks a folder of clips, detects scene boundaries (PySceneDetect), generates a thumbnail-driven HTML preview per clip, and waits for you to mark which scenes to keep before losslessly stream-copying each kept scene into its own output file via FFmpeg. Everything is resumable, failure-isolated, and manifest-driven.

---

## What it's good for

- A folder of **pre-edited or concatenated** videos (camcorder dumps, screen recordings with hard cuts, vlogs spliced from multiple shots) that you want broken back into the individual clips.
- Curating dozens-to-hundreds of clips at a time without re-encoding.
- Batch workflows where one clip failing shouldn't block the others.

## What it's NOT good for

- **A single continuous handheld shot** with no scene cuts (e.g., a 10-second iPhone clip of someone talking). There are no visual discontinuities for a pixel-based detector to find. Tools like [LosslessCut](https://losslesscut.app/) are better for manual point-and-shoot trimming.
- Detecting "interesting moments" by audio/motion. vsplit's detectors are all pixel-based.

---

## Install

Prerequisites: Python 3.11+, [uv](https://github.com/astral-sh/uv), FFmpeg ≥ 5, FFprobe (both on PATH).

```bash
uv venv
uv pip install -e ".[dev]"
uv run vsplit --help
```

To regenerate the test fixtures (one-time):

```bash
uv run python tests/fixtures/make_fixtures.py
```

---

## Quick start

```bash
cd ~/vacation-clips

# 1. Adopt every recognized video file as a workspace.
uv run vsplit scan

# 2. Detect scenes + render previews + open at the review gate.
uv run vsplit run --through preview

# 3. For each clip, open the preview and decide:
open .vsplit/workspaces/*/preview/index.html
# Edit decisions/cuts.json — set "keep": false on scenes you don't want.

# 4. Approve and continue.
uv run vsplit approve "*"
uv run vsplit run

# 5. Your clips:
ls .vsplit/workspaces/*/output/
```

### Single-file shortcut

```bash
uv run vsplit one Birthday.mov              # adopt + preview, opens browser
uv run vsplit one Birthday.mov --auto-approve   # skip review, run end-to-end
```

`one` registers exactly the file you point at — it does **not** scan the rest of the folder.

---

## The 7 stages

Each clip flows through a fixed pipeline. Status of each stage is tracked per workspace in `manifest.json`.

| # | Stage     | Reads                            | Writes                                             | Notes                                                                  |
|---|-----------|----------------------------------|----------------------------------------------------|------------------------------------------------------------------------|
| 1 | `probe`   | source                           | `probe/ffprobe.json`, `probe/keyframes.csv`        | Metadata + keyframe list.                                              |
| 2 | `detect`  | source                           | `detect/scenes.json`                                | PySceneDetect; detector chosen by config.                              |
| 3 | `preview` | `detect/scenes.json`, source     | `preview/index.html`, `preview/images/*.jpg`, `preview/source-proxy.mp4` | Thumbs + browser-playable H.264 proxy (so iPhone HEVC plays in Chrome). |
| 4 | `review`  | `detect/scenes.json`             | `decisions/cuts.json`                              | **Gate.** Status becomes `awaiting_user` until edits or `approve`.     |
| 5 | `plan`    | `decisions/cuts.json`, `probe/keyframes.csv` | `plan/cuts-resolved.json`                          | Snaps each scene start to preceding keyframe.                          |
| 6 | `split`   | `plan/cuts-resolved.json`, source | `output/*.mp4`                                      | `ffmpeg -c copy` — lossless, no re-encode.                              |
| 7 | `verify`  | output                           | `manifest.stages.verify.artifacts.report`           | ffprobe sanity-checks each output file.                                |

Per-stage status: `pending` → `running` → `done` | `failed` | `awaiting_user` | `skipped`.

---

## Global options (must come before the subcommand)

```
uv run vsplit [GLOBAL OPTIONS] <command> [COMMAND OPTIONS]
```

| Option | Default | Purpose |
|---|---|---|
| `--working-dir`, `-w PATH` | `.` | Folder containing your source videos (becomes the "working root"). Resolved to absolute. |
| `--config PATH` | none | Explicit config file path. Highest-priority layer in resolution chain. Errors if missing. |
| `--log-level {debug,info,warn,error}` | `info` | Reserved; per-stage logs always written to `<workspace>/logs/<stage>.log` regardless. |
| `--json` | off | Forces JSON output where supported (`status` honors this). |
| `--version` | — | Print version and exit. |
| `--help` | — | Show help for any command. |

Example combining global + subcommand options:

```bash
uv run vsplit -w "D:\vacation\japan-2025" --json status --awaiting-user
```

---

## Commands

| Command | Purpose |
|---|---|
| `vsplit scan [path]` | Discover and adopt video files |
| `vsplit run [filter]` | Advance pipelines |
| `vsplit status` | Show pipeline progress |
| `vsplit approve <filter>` | Mark workspaces past the review gate |
| `vsplit reset <filter> --from <stage>` | Re-run stages |
| `vsplit clean <filter>` | Delete workspace state (source files untouched) |
| `vsplit one <file>` | Single-file shortcut |
| `vsplit dashboard [--port N]` | Launch the local web dashboard |

### `scan [PATH]`

Walks the working root looking for video files; adopts any new ones (creates workspace + manifest + index entry). Idempotent — files already in the index by hash are silently skipped.

| Option | Default | What it does |
|---|---|---|
| `PATH` (positional) | `--working-dir` | Override the directory to scan for this invocation. |
| `--recursive/--no-recursive` | from config (`scan.recursive: false`) | Walk subdirectories too. |
| `--dry-run` | off | Report what would be adopted without writing anything. |
| `--auto-relocate` | off | When a file's hash matches an existing workspace but the path differs (you renamed/moved the source), update the pointer without prompting. Default is to ask. |

**File-extension filter** comes from config (`scan.extensions`). Defaults: `mp4, mkv, mov, avi, m4v, webm`.

**Edge cases handled:** files inside `.vsplit/` are always skipped; rename detection via partial-hash; replaced-file warning if path matches but hash differs; long-path warning if projected workspace path exceeds 240 chars (Windows MAX_PATH safety).

```bash
uv run vsplit scan
uv run vsplit scan ~/Movies/raw --recursive
uv run vsplit scan --dry-run                    # see what would happen
uv run vsplit scan --auto-relocate              # bulk-update paths after moving files
```

---

### `run [SLUG_GLOB]`

Advances workspaces through the pipeline. Runs the orchestrator: per-stage concurrency limits, crash recovery, failure isolation.

| Option | Default | What it does |
|---|---|---|
| `SLUG_GLOB` (positional) | none | fnmatch-style slug filter (e.g., `"vacation-*"`). Shorthand for `--filter slug=...`. |
| `--only STAGE` | none | Run **only** this stage. Mutually exclusive with `--from`/`--through`. |
| `--from STAGE` | first stage | Start from this stage (inclusive). |
| `--through STAGE` | last stage | Stop after this stage (inclusive). |
| `--filter K=V` | none | Subset of workspaces (see [Filter grammar](#filter-grammar)). Repeat to AND. |
| `--concurrency STAGE=N` | from config | Override concurrency for a stage. Repeat for multiple. Example: `--concurrency split=4`. |
| `--dry-run` | off | Print what would run; no manifests touched. |

`STAGE` is one of: `probe`, `detect`, `preview`, `review`, `plan`, `split`, `verify`.

```bash
uv run vsplit run                                # advance every workspace as far as it can go
uv run vsplit run --through preview              # stop before the review gate
uv run vsplit run --filter status=awaiting_user  # only ones waiting on user
uv run vsplit run "birthday-*" --from plan       # re-run plan/split/verify on matching workspaces
uv run vsplit run --only detect                  # just re-detect (e.g., after threshold change)
uv run vsplit run --concurrency split=4          # 4 ffmpegs in parallel for split
```

Ctrl-C requests a clean shutdown: in-flight stages finish, then the run exits 130. Unfinished workspaces stay `pending` and resume cleanly next time.

---

### `status [SLUG]`

Renders a table of every workspace's stage statuses. With `--json` (the global flag) emits a machine-readable manifest dump.

| Option | What it does |
|---|---|
| `SLUG` (positional) | Limit to exactly this workspace (exact match, no glob). |
| `--awaiting-user` | Only workspaces with at least one stage in `awaiting_user`. |
| `--failed` | Only workspaces with at least one stage in `failed`. |

```bash
uv run vsplit status
uv run vsplit status --awaiting-user
uv run vsplit status birthday-2024
uv run vsplit --json status > snapshot.json     # JSON for tooling
```

---

### `approve [SLUG_GLOB]`

Marks workspaces past the review gate without requiring you to edit `cuts.json`. Writes `approved_at` into the review-stage artifacts. The next `vsplit run` notices this and advances `review → done`.

| Option | What it does |
|---|---|
| `SLUG_GLOB` (positional) | fnmatch-style slug filter. |
| `--filter K=V` | Subset filter (only `status` and `slug` keys honored here — `stage` is silently ignored in this command). |

Skips workspaces whose `review` stage isn't currently `awaiting_user`.

```bash
uv run vsplit approve "*"                       # approve everything
uv run vsplit approve "birthday-*"              # all matching slugs
uv run vsplit approve --filter status=awaiting_user
uv run vsplit approve hike-2024-08              # exact slug
```

---

### `reset [SLUG_GLOB]`

Re-pends stages so they run again. Deletes their on-disk artifacts.

| Option | What it does |
|---|---|
| `SLUG_GLOB` (positional) | fnmatch-style slug filter. |
| `--from STAGE` | Reset STAGE and every stage after it. **One of `--from` or `--all` is required.** |
| `--all` | Reset every stage (probe → verify). |
| `--filter K=V` | Subset filter. Repeat to AND. |
| `--force` | Skip the confirmation prompt that `--all` triggers. |

`STAGE` is one of: `probe`, `detect`, `preview`, `review`, `plan`, `split`, `verify`.

Use this after changing config (re-detect with new threshold), re-opening the review gate, or recovering from corruption.

```bash
uv run vsplit reset birthday-2024 --from detect       # changed detector? re-run from detect on
uv run vsplit reset "*" --from review                 # re-open review for everything
uv run vsplit reset --all --force                     # nuclear: every stage of every workspace
uv run vsplit reset --filter status=failed --from probe   # retry failed ones from scratch
```

---

### `clean [SLUG_GLOB]`

Permanently deletes workspace state. **Source files are never touched** — only the contents of `.vsplit/`.

| Option | What it does |
|---|---|
| `SLUG_GLOB` (positional) | fnmatch-style slug filter. |
| `--all` | Delete every workspace. |
| `--filter K=V` | Subset filter. Repeat to AND. |
| `--force` | Skip the confirmation prompt. |

Cleaned slugs go into `index.retired_slugs` so they won't be re-allocated on next `scan` — re-adopting the same file gets a `-2`, `-3`, etc. suffix. To allow reuse, hand-edit `.vsplit/index.json` and clear the `retired_slugs` array.

```bash
uv run vsplit clean --filter status=done --force      # remove everything fully processed
uv run vsplit clean "test-*" --force                  # remove test workspaces
uv run vsplit clean --all --force                     # nuke everything (sources safe)
```

---

### `one FILE_PATH`

End-to-end shortcut for a single file. Adopts **only** that file (no folder walk), runs through preview, opens the browser, optionally auto-approves and finishes.

| Option | Default | What it does |
|---|---|---|
| `FILE_PATH` (positional, required) | — | Path to a single video file. |
| `--through STAGE` | `preview` | Run through this stage and stop. |
| `--auto-approve` | off | Skip the review gate: run through review, write `approved_at`, finish to verify. |

```bash
uv run vsplit one IMG_1754.MOV                       # preview + open browser
uv run vsplit one IMG_1754.MOV --through detect      # just detect scenes
uv run vsplit one IMG_1754.MOV --auto-approve        # end-to-end with default keep=true
```

Sets the working dir to the file's parent for this invocation (so output lands at `<parent>/.vsplit/workspaces/<slug>/`).

---

### `dashboard`

Launches a localhost web UI for managing workspaces, running commands in bulk, and editing config.

| Option | Default | What it does |
|---|---|---|
| `--port N` | 8765 | TCP port to bind. Must be free. |
| `--host ADDR` | 127.0.0.1 | Bind address. Localhost-only by default; do not expose. |
| `--no-open` | off | Skip auto-opening the browser. |

```bash
uv run vsplit dashboard
# Opens http://127.0.0.1:8765 in your default browser.
```

Features:
- Hybrid collapsible list of every workspace; per-stage status pips.
- Per-row actions: Run, Reset (with stage picker), Clean (confirms), Approve.
- Bulk selection + filter bar (status/stage/slug glob mirrors CLI grammar).
- Inline log viewer for failed stages — click a red pip to open the log drawer.
- Config drawer: structured form for common fields + collapsible raw YAML pane.
- Live status updates via Server-Sent Events (no manual refresh).

Out of scope for v1: inline `cuts.json` editor (use your text editor), multi-user
or LAN access, mobile layouts, light theme.

---

## Filter grammar

Three commands accept `--filter key=value` (`run`, `reset`, `clean`). The grammar is deliberately small: AND-only, no OR/negation/nesting.

```
key   ∈ { slug, status, stage }
value : a literal string interpreted per-key
```

- `slug=PATTERN` — fnmatch glob over slugs. Same as the bare positional argument.
  Examples: `slug=birthday-*`, `slug=*2024*`, `slug=hike-2024-08`.
- `status=VALUE` — workspace has **any** stage in this status.
  Values: `pending`, `running`, `done`, `failed`, `awaiting_user`, `skipped`.
- `stage=NAME` — workspace's *next* non-done stage is this one (i.e. "currently parked at NAME").
  Values: `probe`, `detect`, `preview`, `review`, `plan`, `split`, `verify`.

**Repeat `--filter` to AND multiple predicates.** Order doesn't matter; the bare positional `SLUG_GLOB` is just sugar for `--filter slug=...`.

```bash
# Workspaces parked at the review gate, slug starts with "vacation":
uv run vsplit run --filter status=awaiting_user --filter slug=vacation-*

# Same as above using the positional shortcut:
uv run vsplit run "vacation-*" --filter status=awaiting_user

# Clean only the failures from 2024:
uv run vsplit clean --filter status=failed --filter slug=*-2024-* --force

# Re-run from preview on everything currently parked at detect:
uv run vsplit reset --filter stage=detect --from detect
```

Note: `approve` has a simpler inline parser (predates the shared grammar) that honors only `status=` and `slug=`.

---

## Configuration

Three layers, resolved in this order (later overrides earlier):

1. Built-in defaults (Pydantic schema).
2. `~/.config/vsplit/config.yaml` (POSIX) or `%APPDATA%\vsplit\config.yaml` (Windows) — per-user global.
3. `<working_root>/.vsplit/config.yaml` — per-project.
4. `--config PATH` — explicit override; errors if file is missing.

Unknown keys raise validation errors loud and early. Below is **every** configurable field with its default.

```yaml
# .vsplit/config.yaml

detect:
  detector: content          # content | adaptive | histogram | hash
  threshold: 27.0            # scale depends on the chosen detector (see below)
  min_scene_length: 0.6      # seconds; shorter detected scenes are dropped
  luma_only: false           # ignore chroma in content/adaptive (slight speedup)

preview:
  images_per_scene: 3        # thumbnails per scene; auto-1 for scenes <1.0s
  image_width: 480           # pixels (height auto)
  image_quality: 85          # 1-100, JPEG q (100=best)
  proxy_height: 720          # H.264 proxy height; null/0 disables (saves disk + time)

review:
  default_keep: true         # initial value of "keep" for each scene in cuts.json
                             # set false if you typically discard most scenes (curation mode)
  auto_apply_template: true  # if working_root/cuts.json.template exists, apply on first review

plan:
  keyframe_snap: nearest_preceding   # only mode in v1
  min_segment_length: 1.0    # seconds; shorter resolved segments are dropped from output

split:
  output_container: mp4      # output extension/container

verify:
  duration_tolerance_seconds: 0.5  # tolerance for output-vs-expected duration check

scan:
  extensions: [mp4, mkv, mov, avi, m4v, webm]
  recursive: false           # walk subdirectories

concurrency:
  probe: 4
  detect: 2
  preview: 4
  plan: 8
  split: 1                   # serial by default; disk-I/O bound
  verify: 4
  # Note: "review" is intentionally absent — it runs synchronously.
```

### Detector choice guide

The four detectors interpret `threshold` on **different scales** — changing detector usually means picking a new threshold.

| Detector | Scale | Default in pyscenedetect | What it sees | Good for |
|---|---|---|---|---|
| `content` (default) | 0–100 | 27 | HSV/luma pixel deltas between adjacent frames | Hard cuts, edited footage |
| `adaptive` | 0–10 | 3.0 | Rolling-average–normalized content deltas | Footage with slow lighting drift |
| `histogram` | 0–1 | 0.05 | HSV-histogram chi-squared distance | Gradual transitions, handheld pan/lighting changes |
| `hash` | 0–1 | 0.395 | Perceptual hash distance | Robust against minor noise/shake |

Tuning rules:
- **More scenes than you want?** Raise `threshold`.
- **Fewer scenes than you want?** Lower `threshold`.
- **Detector is missing the obvious boundary but firing on a weak one?** Try `histogram` (different math; often catches what content/adaptive miss on handheld clips).

```yaml
# For handheld iPhone footage with gradual lighting/framing transitions:
detect:
  detector: histogram
  threshold: 0.15
```

---

## Editing `cuts.json`

The review gate writes `decisions/cuts.json` per workspace. Format:

```json
{
  "schema_version": 1,
  "scenes": [
    {
      "id": 1,
      "start": "00:00:00.000",
      "end":   "00:00:03.033",
      "keep": true,
      "label": null,
      "merge_with_next": false
    },
    {
      "id": 2,
      "start": "00:00:03.033",
      "end":   "00:00:05.833",
      "keep": false,
      "label": null,
      "merge_with_next": false
    }
  ]
}
```

| Field | Purpose |
|---|---|
| `id` | Stable scene id from detector. Don't change; don't remove rows — use `keep:false` to drop a scene. |
| `start`, `end` | `HH:MM:SS.fff` strings. You **can** edit these to refine boundaries; `plan` will snap `start` to the preceding keyframe. |
| `keep` | `true` to include this scene in output, `false` to drop it. |
| `label` | Optional human label. Becomes part of the output filename (`<slug>-highlight-NN-<label>.mp4`). Allowed: alphanumeric, `-`, `_` (sanitized to lowercase, repeated `-` collapsed). |
| `merge_with_next` | `true` to fuse this scene into the next one (single output file spanning both). |

**vsplit rejects** files with: bad timecode format, duplicate scene ids, missing or extra ids vs. what `detect/scenes.json` contains, unknown fields. You'll get a clear error pointing at the offending line.

After editing and saving, the next `vsplit run` notices the content-hash changed and advances `review → done`, then continues into `plan` and `split`.

### `cuts.json.template`

If `<working_root>/cuts.json.template` exists, `review` applies it to first-time workspaces automatically and skips the gate. Useful for batch policies like "always drop scene 1 and the final 2 seconds." The template must have matching scene ids; mismatches fail loudly.

---

## Files & directories

```
<working_root>/
  IMG_1754.MOV                                 ← your source (never modified)
  cuts.json.template                           ← optional, applied automatically
  .vsplit/
    config.yaml                                ← project-level config
    index.json                                 ← workspace ledger + retired slugs
    workspaces/
      img-1754/
        manifest.json                          ← per-workspace state of truth
        probe/
          ffprobe.json
          keyframes.csv
        detect/
          scenes.json
        preview/
          index.html
          source-proxy.mp4                     ← H.264 proxy for browser playback
          images/
            scene-001-01.jpg
            ...
        decisions/
          cuts.json                            ← edit this
        plan/
          cuts-resolved.json
        output/
          img-1754-highlight-01.mp4            ← your final clips
          img-1754-highlight-02-cake.mp4
        logs/
          <stage>.log                          ← one per stage run
```

### Slug rules

- Lowercased filename stem.
- Windows-reserved characters (`<>:"|?*\/`) and control chars stripped.
- Spaces and underscores → hyphen; runs of hyphens collapsed.
- Names matching reserved Windows device names (`CON`, `NUL`, `COM1`, `LPT1`, …) prefixed with `_`.
- Truncated to 64 characters; empty results become `_unnamed`.
- On collision (case-insensitive, includes retired slugs), `-2`, `-3`, … is appended (cap 99).

---

## Troubleshooting

**Preview thumbnails missing / `ff_frame_thread_encoder_init failed`** — Your source is 10-bit HEVC (iPhone default). vsplit forces 8-bit conversion in the filter chain, so this should be fixed; if you still see it, your ffmpeg build may be unusual — verify with `ffmpeg -version`.

**Video player in the preview shows nothing** — Browsers can't decode HEVC reliably. The H.264 proxy (`preview/source-proxy.mp4`) is generated by default. If you've disabled it via `proxy_height: null`, click the "Source: …" link below the player to open the original in a system player like VLC.

**Detector finds 0 scenes** — Your footage probably has no hard cuts (a single continuous shot). Try `detector: histogram` with `threshold: 0.15` for handheld clips. If still 0, the footage genuinely lacks scene boundaries; vsplit is the wrong tool.

**Cut lands ~1s before the visual transition** — That's the keyframe-snap slack. Stream-copy can only cut at keyframes; we snap backward (lossless) rather than forward (would lose content). Inspect `cut at X → Y · detected Z (snapped back …)` in the preview header.

**Run was interrupted (Ctrl-C, power loss, etc.)** — `vsplit run` will see `running` stages on next launch, call each stage's `recover()` to clean partials (or validate them in the case of `split`), and resume.

**One workspace fails; rest of batch keeps going.** That's the design. `vsplit status --failed` shows you which ones; `vsplit reset --filter status=failed --from <stage>` retries.

---

## Windows notes

- Keep your working folder near the drive root — long paths can exceed Windows' 260-char MAX_PATH. The tool warns at scan time if the projected workspace path exceeds 240 chars.
- Exclude the working folder from real-time antivirus scanning during heavy operations (split + verify spawn many ffprobe/ffmpeg subprocesses).
- Source paths are stored relative to the working root; if you move the whole folder, paths still resolve. If you rename individual files, run `vsplit scan --auto-relocate`.

---

## License

(not yet)
