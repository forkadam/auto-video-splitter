# vsplit — Implementation Spec

> Local, automated, scene-aware video splitter with a plug-and-play stage pipeline and human-in-the-loop review.
>
> Status: draft v0.1. Name `vsplit` is a placeholder.

---

## 1. Overview

`vsplit` is a command-line tool that takes one video file or a folder of video files and splits each into scenes using automated detection (PySceneDetect) and lossless cutting (FFmpeg). The user reviews detected scenes between detection and splitting; everything else is automated.

The design is two layers:

- **Orchestrator** — batch concerns: discovery, workspace adoption, dispatching, concurrency limits, status reporting, failure isolation.
- **Per-file pipeline** — single-video concerns: probe, detect, preview, review, plan, split, verify. Each stage is a pluggable module operating on one workspace.

Source files are never modified. All derived state lives under `.vsplit/` inside the working folder.

## 2. Goals and non-goals

### Goals

- Local-only operation; no network calls in the hot path.
- Lossless splits by default (stream-copy, snapping to keyframes).
- High-quality re-encode as an opt-in alternative when frame accuracy matters.
- Process a single video or 50+ in a batch with the same commands.
- Resumable across every stage and across process restarts.
- Human review of detected scenes before splitting, with edits as a plain JSON file.
- Plug-in stages: drop a module in, add a line to config, it runs.
- Works identically on Windows, macOS, Linux. Primary target is Windows.
- Pure CLI. No GUI. HTML preview pages are acceptable static artifacts.

### Non-goals

- No built-in desktop GUI.
- No cloud upload, sync, or remote storage.
- No transcoding, color grading, audio editing, or compositing.
- No real-time or live-stream processing.
- No support for DRM-protected sources.

## 3. Architecture

### 3.1 Two layers

```
                  vsplit CLI
                       |
              +--------+--------+
              |                 |
       Orchestrator        (one process)
              |
  +-----------+-----------+-----------+
  |           |           |           |
Pipeline   Pipeline    Pipeline    Pipeline    ...  N independent pipelines
(goldeneye)(casino-r..)(dr-no)    (workspace4)      
```

The orchestrator and pipelines run in the same Node process; the separation is conceptual, not OS-level.

### 3.2 Source of truth

| Concern                          | Source of truth                          |
|----------------------------------|------------------------------------------|
| Which sources are adopted        | `.vsplit/index.json`                     |
| State of one workspace           | `.vsplit/workspaces/<slug>/manifest.json`|
| User intent for cuts             | `.vsplit/workspaces/<slug>/decisions/cuts.json` |
| Pipeline configuration           | `.vsplit/config.yaml`                    |
| Derived artifacts                | The workspace folder tree                |

Stages read inputs from the workspace and write outputs to the workspace. The manifest tracks progress. Nothing else carries state.

### 3.3 Pipeline order (default)

```
probe -> detect -> preview -> review -> plan -> split -> verify
```

`review` is a halt gate: it pre-populates `decisions/cuts.json` from detected scenes, sets its own status to `awaiting_user`, and exits. The next `vsplit run` checks the gate and either continues (if user-edited) or skips that workspace.

## 4. Working folder layout

```
working/                                  # user's chosen working folder
  GoldenEye (1995).mkv                    # source files, untouched
  Casino Royale.mp4
  Dr. No [1962].avi
  ...
  vsplit.config.yaml                      # optional, project-local config
  .vsplit/
    index.json                            # orchestrator state
    config.yaml                           # effective config (resolved)
    workspaces/
      goldeneye/
        manifest.json                     # per-file pipeline state
        probe/
          ffprobe.json
          keyframes.csv
        detect/
          scenes.csv
          scenes.json                     # normalized for downstream stages
        preview/
          images/
            scene-001-01.jpg
            scene-001-02.jpg
            scene-001-03.jpg
            ...
          index.html                      # contact sheet, opens in browser
          scenes.md                       # vim-browsable summary
        decisions/
          cuts.json                       # USER EDITS THIS
        plan/
          cuts-resolved.json              # cuts snapped to keyframes
        output/
          goldeneye-scene-001.mp4
          goldeneye-scene-002.mp4
          ...
        logs/
          probe.log
          detect.log
          preview.log
          plan.log
          split.log
          verify.log
      casino-royale/
        ...
```

Source files are referenced by relative path from the working folder; no symlinks, no copies, no hardlinks.

## 5. Manifest schema

`.vsplit/workspaces/<slug>/manifest.json`:

```jsonc
{
  "schema_version": 1,
  "id": "goldeneye",                      // workspace slug
  "created_at": "2026-05-20T10:14:22Z",
  "updated_at": "2026-05-20T11:02:08Z",

  "source": {
    "relative_path": "GoldenEye (1995).mkv",  // relative to working/ root
    "size_bytes": 8472193024,
    "mtime": "2024-11-03T14:22:18Z",
    "partial_hash": "sha256:a3f4...c89e"      // first 1MB + last 1MB + size
  },

  "metadata": {                            // written by probe
    "duration_seconds": 7920.521,
    "container": "matroska",
    "video_codec": "h264",
    "audio_codec": "aac",
    "fps": 23.976,
    "width": 1920,
    "height": 1080,
    "keyframe_count": 793
  },

  "stages": {
    "probe":   { "status": "done", "started_at": "...", "completed_at": "..." },
    "detect":  { "status": "done", "scenes_found": 1247, "detector": "adaptive" },
    "preview": { "status": "done", "images_generated": 3741 },
    "review":  { "status": "awaiting_user", "set_at": "..." },
    "plan":    { "status": "pending" },
    "split":   { "status": "pending" },
    "verify":  { "status": "pending" }
  },

  "config_snapshot": {                     // config at last run, for reproducibility
    "detect": { "algorithm": "detect-adaptive", "threshold": 27.0 },
    "split":  { "strategy": "stream_copy" }
  }
}
```

Stage `status` values: `pending`, `running`, `done`, `failed`, `awaiting_user`, `skipped`.

When `status` is `failed`, the stage block includes `error: { message, exit_code, log_path }`.

## 6. Index schema

`.vsplit/index.json`:

```jsonc
{
  "schema_version": 1,
  "working_root": ".",                     // relative or absolute
  "workspaces": {
    "goldeneye": {
      "source_relative_path": "GoldenEye (1995).mkv",
      "size_bytes": 8472193024,
      "partial_hash": "sha256:a3f4...c89e",
      "adopted_at": "2026-05-20T10:14:22Z"
    }
  },
  "tombstones": []                         // slugs of deleted workspaces, prevents collision
}
```

The index supports:

- Lookup by source path → workspace slug.
- Lookup by hash → detect renames/moves.
- Collision detection when slugs would clash.

## 7. Configuration

`.vsplit/config.yaml` (with sensible defaults if file is missing):

```yaml
# Pipeline composition. Stages run in this order.
pipeline:
  - probe
  - detect
  - preview
  - review
  - plan
  - split
  - verify

# Per-stage configuration.
stages:
  detect:
    algorithm: detect-adaptive      # detect-adaptive | detect-content | detect-threshold
    threshold: 27.0
    min_scene_length: 0.6           # seconds
    luma_only: false

  preview:
    images_per_scene: 3
    image_width: 480
    image_quality: 85
    generate_html: true
    generate_markdown: true

  review:
    auto_apply_template: true       # use cuts.json.template if present
    skip_when_template_complete: false

  plan:
    keyframe_snap: nearest_preceding   # nearest_preceding | nearest | none
    min_segment_length: 1.0         # seconds; merge smaller into neighbor

  split:
    strategy: stream_copy           # stream_copy | reencode | smart_cut
    reencode:
      crf: 17
      preset: slow
      video_codec: libx264
      audio_codec: aac
    output_template: "$VIDEO_NAME-scene-$SCENE_NUMBER.$EXT"
    output_container: mp4

  verify:
    check_outputs_exist: true
    check_durations: true
    duration_tolerance_seconds: 0.5

# Orchestrator-level concurrency.
concurrency:
  probe: 4
  detect: 2          # CPU-heavy, mostly single-threaded per video
  preview: 4
  plan: 8
  split: 2           # I/O bound for stream-copy
  verify: 4

# Discovery rules for `vsplit scan`.
scan:
  extensions: [mp4, mkv, avi, mov, m4v, webm, flv, wmv]
  recursive: false
  exclude_patterns: [".vsplit/**", "**/.*"]

# Custom stage modules (relative paths or npm package names).
custom_stages: []
```

Resolution order: built-in defaults → `.vsplit/config.yaml` → `vsplit.config.yaml` at working root → CLI flags.

## 8. Stage interface

Every stage exports a single object conforming to:

```typescript
// src/core/types.ts

export interface Stage {
  name: string;
  description?: string;
  dependsOn?: string[];           // stage names that must be `done` first
  produces?: string[];            // workspace-relative paths written
  consumes?: string[];            // workspace-relative paths read

  run(ctx: StageContext): Promise<StageResult>;
}

export interface StageContext {
  workspaceDir: string;           // absolute path to workspace
  workingRoot: string;            // absolute path to working folder root
  sourcePath: string;             // absolute path, resolved from manifest
  manifest: Manifest;             // mutable; persisted after stage returns
  config: ResolvedConfig;
  logger: StageLogger;            // writes to workspace/logs/<stage>.log
  exec: typeof import("execa").execa;
  signal: AbortSignal;            // honor for cancellation
}

export interface StageResult {
  artifacts?: Record<string, unknown>;   // merged into manifest.stages[name]
  status?: "done" | "awaiting_user" | "skipped";
}
```

### Stage lifecycle

For each workspace × stage combination, the orchestrator:

1. Checks `manifest.stages[stage].status`. If `done`, skip. If `awaiting_user`, treat as gate (see §10).
2. Checks `dependsOn`; if any dependency is not `done`, skip this workspace for now.
3. Sets status to `running`, persists manifest, calls `stage.run(ctx)`.
4. On success: merges `result.artifacts` into manifest, sets status from `result.status` (default `done`), persists.
5. On error: sets status to `failed`, writes error block, persists. Other workspaces continue.

### Custom stages

A custom stage is a `.ts`/`.js` module exporting a default `Stage` object. To register:

```yaml
custom_stages:
  - ./stages/audio-silence-detect.ts
  - my-org-vsplit-whisper-stage          # npm package

pipeline:
  - probe
  - detect
  - audio-silence-detect                 # merges audio-derived cuts with scene cuts
  - preview
  - review
  - plan
  - split
  - verify
```

## 9. Built-in stages

### 9.1 probe

- **Purpose**: extract metadata + full keyframe list for downstream stages.
- **Depends on**: nothing.
- **Produces**: `probe/ffprobe.json`, `probe/keyframes.csv`, populates `manifest.metadata`.
- **Commands**:
  - `ffprobe -v error -print_format json -show_streams -show_format <src>`
  - `ffprobe -v error -select_streams v:0 -skip_frame nokey -show_frames -show_entries frame=pts_time -of csv=p=0 <src>`
- **Failure modes**: source unreadable, codec unidentifiable, no video stream.

### 9.2 detect

- **Purpose**: run PySceneDetect to find scene boundaries.
- **Depends on**: `probe`.
- **Produces**: `detect/scenes.csv`, `detect/scenes.json`.
- **Command**: `scenedetect --input <src> --output <workspace>/detect <detector> list-scenes --filename scenes`
- **Detectors**: `detect-adaptive` (default; handles motion well), `detect-content` (classic), `detect-threshold` (for fade-to-black sources).
- **scenes.json format**:
  ```jsonc
  {
    "detector": "detect-adaptive",
    "config": { "threshold": 27.0 },
    "scenes": [
      { "id": 1, "start_time": 0.0,    "end_time": 12.345, "start_frame": 0,    "end_frame": 296 },
      { "id": 2, "start_time": 12.345, "end_time": 28.110, "start_frame": 296,  "end_frame": 674 }
    ]
  }
  ```

### 9.3 preview

- **Purpose**: generate thumbnails per scene and browsable preview pages.
- **Depends on**: `detect`.
- **Produces**: `preview/images/scene-NNN-MM.jpg`, `preview/index.html`, `preview/scenes.md`.
- **Behavior**:
  - Calls `scenedetect --input <src> save-images -n <N>` for N images per scene.
  - Generates `index.html`: a CSS-grid contact sheet with three thumbnails per scene, scene number, start/end timecodes, click-to-seek `<video>` tag using `#t=start,end` fragment.
  - Generates `scenes.md`: line per scene with timecodes and thumbnail paths, vim-browsable.

### 9.4 review

- **Purpose**: human-in-the-loop gate.
- **Depends on**: `preview`.
- **Produces**: `decisions/cuts.json` (pre-populated from `detect/scenes.json`).
- **Behavior**:
  - If `decisions/cuts.json` does not exist: create it from detected scenes (all `keep: true`), set status to `awaiting_user`, exit.
  - If `decisions/cuts.json` exists and was modified after stage start: set status to `done`.
  - If `auto_apply_template: true` and `working/cuts.json.template` exists: apply template, set status to `done` directly.
- **cuts.json format**:
  ```jsonc
  {
    "schema_version": 1,
    "split_strategy": "stream_copy",      // overrides config
    "output_template": null,              // null = inherit from config
    "scenes": [
      {
        "id": 1,
        "start": "00:00:00.000",
        "end":   "00:02:34.500",
        "keep": true,
        "label": "intro",                 // optional, used in filename if present
        "merge_with_next": false
      },
      {
        "id": 2,
        "start": "00:02:34.500",
        "end":   "00:05:12.000",
        "keep": false                     // dropped from output
      }
    ]
  }
  ```

### 9.5 plan

- **Purpose**: resolve user cuts against keyframes; compute final ffmpeg cut points.
- **Depends on**: `review`, `probe`.
- **Produces**: `plan/cuts-resolved.json`.
- **Behavior**:
  - For each kept scene, snap start time to nearest preceding keyframe (`nearest_preceding`) if `strategy: stream_copy`.
  - For `strategy: reencode`, pass through cut times unchanged.
  - For `strategy: smart_cut`, mark which segments need re-encoding (the GOP straddling each cut) and which can stream-copy.
  - Apply `merge_with_next` to combine adjacent scenes into one output.
  - Drop scenes with `keep: false`.
  - Apply `min_segment_length` to merge tiny segments into neighbors.
- **cuts-resolved.json format**:
  ```jsonc
  {
    "scenes": [
      {
        "id": 1,
        "label": "intro",
        "requested_start": 0.0,
        "requested_end": 154.5,
        "actual_start": 0.0,              // snapped to keyframe
        "actual_end": 154.5,
        "drift_seconds": 0.0,
        "strategy": "stream_copy",
        "output_filename": "goldeneye-scene-001-intro.mp4"
      }
    ]
  }
  ```

### 9.6 split

- **Purpose**: execute ffmpeg to produce output files.
- **Depends on**: `plan`.
- **Produces**: `output/*.mp4` (or configured container).
- **Behavior**:
  - For each segment in `plan/cuts-resolved.json`, run ffmpeg.
  - Stream-copy: `ffmpeg -ss <start> -to <end> -i <src> -map 0 -c copy -avoid_negative_ts make_zero <out>`
  - Re-encode: `ffmpeg -ss <start> -to <end> -i <src> -map 0 -c:v libx264 -crf 17 -preset slow -c:a aac <out>`
  - Honors concurrency limit from config.
- **Failure modes**: disk full, output path collision, invalid codec for chosen container.

### 9.7 verify

- **Purpose**: sanity check the outputs.
- **Depends on**: `split`.
- **Produces**: `manifest.stages.verify.artifacts.report`.
- **Checks**:
  - Every output file in `cuts-resolved.json` exists.
  - Each output file is readable by `ffprobe`.
  - Each output duration is within `duration_tolerance_seconds` of expected.
  - Sum of output durations approximately equals sum of kept scene durations.

## 10. CLI reference

### Global flags

```
--working-dir, -w <path>      # default: cwd
--config <path>                # override config location
--log-level <level>            # debug | info | warn | error (default: info)
--no-color                     # disable terminal colors
--json                         # machine-readable output where applicable
```

### Commands

#### `vsplit scan [path]`

Discover videos in the working folder and adopt new ones into workspaces. Idempotent.

```
vsplit scan                    # scan current directory
vsplit scan ./working
vsplit scan --recursive
vsplit scan --extensions mp4,mkv
vsplit scan --dry-run          # print what would be adopted; no writes
```

Adoption process per file:
1. Compute size + mtime + partial hash.
2. Look up in index by hash, then by path.
3. New → slugify name, check collision, create workspace, register in index.
4. Hash matches but path differs → offer to update pointer (interactive) or update silently (`--auto-relocate`).
5. Path matches but hash differs → source replaced; flag (do not auto-overwrite).

#### `vsplit run [filter]`

Advance one or more workspaces through the pipeline.

```
vsplit run                                # advance every workspace as far as possible
vsplit run goldeneye                      # one workspace by slug
vsplit run --filter "status:awaiting_user"
vsplit run --filter "status:failed"
vsplit run --filter "stage:detect"
vsplit run --through preview              # stop after preview for everyone
vsplit run --from plan                    # resume from plan onward
vsplit run --only detect                  # run just this stage
vsplit run --concurrency detect=4,split=1
vsplit run --dry-run                      # print planned actions, no execution
```

Filter expressions: `status:<value>`, `stage:<name>`, `slug:<glob>`, joined by `,` (AND) or `|` (OR). Bare argument is treated as a slug glob.

#### `vsplit status [filter]`

Show pipeline status.

```
vsplit status                  # all workspaces, table format
vsplit status goldeneye        # one workspace, detailed
vsplit status --failed
vsplit status --awaiting-user
vsplit status --json
```

Default table format:

```
SLUG               PROBE  DETECT  PREVIEW  REVIEW  PLAN  SPLIT  VERIFY
goldeneye           ok     ok      ok       wait    -     -      -
casino-royale       ok     ok      ok       ok      ok    ok     ok
dr-no               ok     FAIL    -        -       -     -      -

3 workspaces · 1 done · 1 awaiting user · 1 failed
```

#### `vsplit reset <workspace> [--from <stage>]`

Invalidate stages so they re-run on next `vsplit run`.

```
vsplit reset goldeneye --from detect      # re-run detect and everything after
vsplit reset goldeneye --all              # reset entire pipeline
vsplit reset --filter "status:failed" --from detect
```

Reset deletes artifacts produced by reset stages and clears their manifest entries.

#### `vsplit relocate <workspace> <new-source-path>`

Update the source pointer when a file has been moved or renamed outside vsplit's view.

```
vsplit relocate goldeneye "GoldenEye 1995 [REMASTERED].mkv"
```

#### `vsplit clean <workspace>`

Delete a workspace entirely (manifest, all artifacts). Source file is never touched.

```
vsplit clean goldeneye
vsplit clean --all
vsplit clean --filter "status:done"      # cleanup completed work
```

Confirmation prompt unless `--force`.

#### `vsplit doctor`

Diagnose environment and config.

```
vsplit doctor
```

Checks:
- `ffmpeg`, `ffprobe`, `scenedetect` are on PATH and a known-good version.
- Working folder is writable.
- Path lengths are within Windows safe limits.
- Disk space available is plausible vs source sizes.
- Config file parses; custom stage modules load.

#### `vsplit config show | edit | path`

```
vsplit config show             # print resolved config
vsplit config edit             # open in $EDITOR (defaults to vim on POSIX, notepad on Windows)
vsplit config path             # print path to config file
```

## 11. Workflows

### 11.1 Single video, interactive

```
cd ~/videos
vsplit scan
vsplit run --through preview
# open .vsplit/workspaces/goldeneye/preview/index.html in browser
# edit .vsplit/workspaces/goldeneye/decisions/cuts.json
vsplit run
```

### 11.2 Batch, overnight detection

```
cd /mnt/storage/movies
vsplit scan
vsplit run --through preview          # leave running overnight
# next morning:
vsplit status --awaiting-user         # see all parked workspaces
# review each: open preview/index.html, edit cuts.json
vsplit run                            # finishes plan/split/verify on all
```

### 11.3 Batch, template-driven (no manual review)

```
# create working/cuts.json.template with a default split strategy
cd ./working
vsplit scan
vsplit run                            # review stage auto-applies template
```

Useful when all videos in the batch follow the same pattern (e.g. "keep every detected scene, output as separate files").

### 11.4 Failure recovery

```
vsplit status --failed
# goldeneye failed at detect; inspect:
cat .vsplit/workspaces/goldeneye/logs/detect.log
# fix root cause (corrupted source, missing codec, etc.)
vsplit run goldeneye --from detect
```

### 11.5 Re-run with different detection parameters

```
# edit .vsplit/config.yaml: detect.threshold: 32.0
vsplit reset --all --from detect
vsplit run --through preview
```

### 11.6 Adding a custom stage

```
# write stages/audio-silence-detect.ts exporting a Stage
# edit .vsplit/config.yaml: insert "audio-silence-detect" after "detect" in pipeline
# add to custom_stages list
vsplit reset --all --from audio-silence-detect
vsplit run
```

## 12. Concurrency and failure isolation

### Concurrency model

Each stage type has a configurable concurrency limit (via `p-limit`). The dispatcher iterates workspaces and asks each: "what is your next runnable stage?" Runnable means status is `pending` and all `dependsOn` stages are `done`. The dispatcher schedules runnable work into the appropriate stage's limiter.

A workspace can have only one stage running at a time. Different workspaces' stages can run concurrently within their limits.

### Failure isolation

A stage failure marks `manifest.stages[name].status = "failed"`, writes the error block, and returns control to the dispatcher. The dispatcher continues with other workspaces. Stages downstream of a failed stage become unreachable for that workspace until the failure is resolved.

### Cancellation

SIGINT/SIGTERM/Ctrl-C triggers graceful shutdown:
1. Dispatcher stops scheduling new stages.
2. Currently running stages receive `AbortSignal` via `ctx.signal`.
3. Stages should propagate to subprocess (`execa` does this automatically).
4. Manifest entries for interrupted stages are marked back to `pending` if no partial state.
5. Process exits with code 130.

Re-running picks up where each workspace left off.

## 13. Windows-specific behavior

### Source file handling

No symlinks. No hardlinks. Source path stored as relative path in manifest. Stages resolve to absolute path at runtime via `path.resolve(workingRoot, manifest.source.relative_path)`.

### Slug generation

Workspace slugs derived from source filename with these rules:
1. Strip extension.
2. Lowercase.
3. Replace whitespace and `_` with `-`.
4. Strip Windows-reserved characters: `< > : " / \ | ? *`.
5. Strip trailing dots and spaces.
6. If result matches a Windows reserved name (`con`, `prn`, `aux`, `nul`, `com1`–`com9`, `lpt1`–`lpt9`), prepend `_`.
7. Collapse repeated `-` into one.
8. Truncate to 64 characters.
9. If collision with existing slug in index (case-insensitive comparison), append `-2`, `-3`, etc.

### Path length

Default `MAX_PATH = 260` on Windows. The tool:
1. At scan time, estimates maximum projected path length: `<working>\.vsplit\workspaces\<slug>\preview\images\scene-XXX-XX.jpg`.
2. If > 240, warns and suggests shorter working path or shorter source filenames.
3. `vsplit doctor` flags this proactively.
4. On Windows 10+ with long path support, this is a non-issue but still surfaced.

### Subprocess invocation

All subprocess calls go through `execa(<command>, [<args>])` with arguments as an array, never as a shell string. This handles quoting of paths with spaces, CJK characters, parentheses, brackets correctly across PowerShell, cmd, and Git Bash.

### Console encoding

CLI startup detects Windows + non-UTF-8 console code page and either:
- Sets the process code page to 65001 (`chcp 65001`) for the duration; or
- Writes through Node's UTF-8 streams and warns if mojibake is likely.

### Anti-virus

Documented but not coded around: the preview stage generating hundreds of thumbnails per video can be slowed by real-time AV scanning. Doc recommends excluding `.vsplit\` from AV scope.

### Line endings

All text files (JSON, YAML, Markdown) written with LF line endings. Acceptable to every editor including modern Notepad.

## 14. Tech stack

### Required runtime dependencies

- **Node.js** ≥ 20 LTS.
- **FFmpeg** + **FFprobe** ≥ 5.0 on PATH.
- **PySceneDetect** ≥ 0.6.4 on PATH (`scenedetect` command).
- **Python** ≥ 3.9 (required by PySceneDetect).

### Node packages

| Package    | Purpose                                          |
|------------|--------------------------------------------------|
| `execa`    | Cross-platform subprocess invocation             |
| `zod`      | Schema validation for manifest, config, cuts.json|
| `clipanion`| CLI framework with subcommands                   |
| `listr2`   | Terminal progress UI; falls back to plain logs   |
| `p-limit`  | Concurrency limits per stage type                |
| `chalk`    | Terminal coloring                                |
| `cli-table3`| Status table rendering                          |
| `yaml`     | Config file parsing                              |
| `pino`     | Structured logging to per-stage log files        |
| `chokidar` | Optional: watch mode for development             |

### Project structure

```
vsplit/
  src/
    cli/                    # clipanion commands
      scan.ts
      run.ts
      status.ts
      reset.ts
      relocate.ts
      clean.ts
      doctor.ts
      config.ts
    core/
      types.ts              # Stage, Manifest, Config types
      orchestrator.ts       # discovery, dispatch, concurrency
      pipeline.ts           # per-workspace stage runner
      manifest.ts           # read/write/migrate
      index.ts              # working-folder index
      slugify.ts            # Windows-safe slug generation
      paths.ts              # cross-platform path helpers
      logging.ts
    stages/
      probe.ts
      detect.ts
      preview.ts
      review.ts
      plan.ts
      split.ts
      verify.ts
    schemas/
      manifest.schema.ts
      config.schema.ts
      cuts.schema.ts
  bin/
    vsplit                  # entry point
  package.json
  tsconfig.json
  README.md
```

## 15. Open questions

- **Hash strategy**: partial hash (size + first/last 1MB) is fast but theoretically collidable. Is this an acceptable tradeoff vs. full sha256 (slow on 50 × 5GB files)? Lean: yes, partial hash with size validation is enough.
- **Smart-cut implementation**: re-encoding only the GOP at each cut point is complex; reference implementation is LosslessCut's source. Defer to v2.
- **Master dashboard**: an aggregate HTML view across all workspaces would help batch review at scale. Defer to v2.
- **Watch mode**: `vsplit watch` could observe the working folder and auto-adopt new files. Defer.

## 16. Future work

- **TransNetV2 detector** as an optional stage replacing `detect`. Better accuracy on dissolves, requires PyTorch.
- **Whisper transcription stage** producing per-scene captions, used in `preview/index.html` and as scene labels.
- **Smart-cut split strategy** for frame-accurate lossless cuts.
- **Audio silence detection stage** that merges with scene cuts for better breakpoints in interview/lecture footage.
- **Output muxing options** beyond MP4 (MKV with chapters, segmented HLS).
- **Per-workspace config overrides** (`.vsplit/workspaces/<slug>/config.yaml`).
- **Aggregate dashboard HTML** for batch review.
- **JSON-RPC server mode** for integration with editors or external UIs.

---

## Appendix A: Example end-to-end output

For input `working/GoldenEye (1995).mkv`:

```
working/
  GoldenEye (1995).mkv
  .vsplit/
    config.yaml
    index.json
    workspaces/
      goldeneye/
        manifest.json
        probe/
          ffprobe.json                     # 2 KB
          keyframes.csv                    # 793 entries
        detect/
          scenes.csv
          scenes.json                      # 1247 scenes
        preview/
          images/                          # 3741 JPEGs
          index.html
          scenes.md
        decisions/
          cuts.json                        # user-edited
        plan/
          cuts-resolved.json               # 24 final segments
        output/
          goldeneye-scene-001-intro.mp4
          goldeneye-scene-002.mp4
          ...
          goldeneye-scene-024-credits.mp4
        logs/
          probe.log  detect.log  preview.log
          plan.log   split.log   verify.log
```

## Appendix B: cuts.json minimum viable example

```json
{
  "scenes": [
    { "id": 1, "start": "00:00:00.000", "end": "00:02:34.500", "keep": true },
    { "id": 2, "start": "00:02:34.500", "end": "00:05:12.000", "keep": false }
  ]
}
```

Everything else defaults from config.
