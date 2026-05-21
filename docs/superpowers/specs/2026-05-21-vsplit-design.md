# vsplit — Redesign Spec (v0.1)

> A first-principles redesign of the local scene-aware video splitter. Supersedes the architectural decisions in `docs/vsplit-design-doc.md` and `docs/vsplit-implementation-spec.md`; those remain as historical context.
>
> Status: draft v0.1, dated 2026-05-21.

---

## 1. Shape and goals

`vsplit` is a Python CLI that helps a single user curate scene-based highlights from batches of personal video — vacation and event footage, typically 50–200 short-to-medium clips per batch. Runs locally on Windows (primary) and macOS (occasional). No GUI in v1.

### 1.1 Architecture in one sentence

Two layers: an **orchestrator** dispatches per-file pipelines under per-stage concurrency limits; each **pipeline** runs an ordered sequence of stages that communicate through files in a workspace folder, with a `manifest.json` as the single source of truth for stage progress.

### 1.2 What changes vs. the prior design docs

- **Language is Python**, not TypeScript. PySceneDetect is used as a library (no subprocess for `detect`). One runtime to install on Windows. A future Bun-based review UI can read the same JSON files; the filesystem-as-bus contract makes that drop-in.
- **Scope is sharp.** No plug-in stages, no `custom_stages` config, one detector (adaptive), one split strategy (stream-copy), no smart-cut, no `doctor`, no `relocate`, no watch mode. Resetting is "delete artifacts and re-run."
- **Curation-friendly grafts.** `vsplit one <file>` single-file shortcut; `default_keep` config flag so detected scenes can pre-populate as drops (for "keep few, drop many" workflows like vacation footage).
- **Review gate has a real contract.** Content-hash change detection on `decisions/cuts.json` instead of mtime, plus an explicit `vsplit approve <slug>` command.
- **Manifest schema cleaned up.** Stage lifecycle (status, timestamps) separated from stage artifacts (counts, configs) — no more conflation.
- **Dispatcher state machine made explicit.** No ambiguity about when a workspace can advance or when stages run in parallel.

### 1.3 Goals (v1)

- One trip's worth of clips (~150 files, ~50 GB) end-to-end on Windows in an evening.
- Resumable at every stage; crashes never lose work that didn't need to be re-done.
- Failure isolation: one broken file stops only itself.
- Curation UX painless enough that the user actually uses it.

### 1.4 Non-goals (v1)

- Plug-in stages, multi-detector, re-encode/smart-cut, doctor, relocate, watch mode.
- Built-in TUI/GUI/hotkey shuttle. A v1.5 Bun-based review UI is anticipated but out of v1 scope.
- Cross-batch operations (master dashboards, aggregate views).
- DRM-protected sources.

---

## 2. Pipeline stages

Seven stages, fixed order per workspace. Each is a Python class with a tight contract; no plug-in mechanism in v1 (but the interface is stable enough that adding a stage in v2 is mechanical).

### 2.1 Stage list

| Stage     | Depends on        | Reads                                          | Writes                                                          | Subprocess                     |
|-----------|-------------------|------------------------------------------------|-----------------------------------------------------------------|--------------------------------|
| `probe`   | –                 | source                                         | `probe/ffprobe.json`, `probe/keyframes.csv`, `manifest.metadata`| `ffprobe`                      |
| `detect`  | `probe`           | source, `manifest.metadata`                    | `detect/scenes.json`                                            | PySceneDetect (in-process)     |
| `preview` | `detect`          | source, `detect/scenes.json`                   | `preview/images/*.jpg`, `preview/index.html`                    | `ffmpeg` (frame extraction)    |
| `review`  | `preview`         | `detect/scenes.json`                           | `decisions/cuts.json` (initial); status `awaiting_user`         | –                              |
| `plan`    | `review`, `probe` | `decisions/cuts.json`, `probe/keyframes.csv`   | `plan/cuts-resolved.json`                                       | –                              |
| `split`   | `plan`            | source, `plan/cuts-resolved.json`              | `output/*.mp4`                                                  | `ffmpeg -c copy`               |
| `verify`  | `split`           | `plan/cuts-resolved.json`, `output/*`          | report in `manifest.stages.verify.artifacts`                    | `ffprobe`                      |

### 2.2 Stage contract (Python)

```python
from typing import Literal
from dataclasses import dataclass, field
from pathlib import Path
import threading, logging
import shutil

@dataclass
class StageContext:
    workspace_dir: Path
    working_root: Path
    source_path: Path            # resolved absolute
    manifest: "Manifest"         # mutable; orchestrator persists after stage returns
    config: "ResolvedConfig"
    logger: logging.Logger       # writes to logs/<stage>.log
    cancel_event: threading.Event

@dataclass
class StageResult:
    artifacts: dict = field(default_factory=dict)
    status: Literal["done", "awaiting_user"] = "done"

class Stage:
    """Base class. Subclasses override run(); recover() if non-default cleanup needed."""
    name: str
    depends_on: list[str]
    output_subdir: str | None    # e.g. "probe", "detect"; None for stages with no own dir

    def run(self, ctx: StageContext) -> StageResult:
        raise NotImplementedError

    def recover(self, ctx: StageContext) -> None:
        """Called on startup if the stage was found in status=running.

        Default: delete the stage's output_subdir and reset status to pending.
        Override (e.g. in `split`) if partial outputs may be salvageable.
        """
        if self.output_subdir is not None:
            shutil.rmtree(ctx.workspace_dir / self.output_subdir, ignore_errors=True)
        ctx.manifest.stages[self.name].status = "pending"
        ctx.manifest.stages[self.name].artifacts = {}
```

The `recover` method is the contract referenced by §5.6's crash-recovery flow. Most stages use the default; `split` overrides it to validate already-written output files individually rather than discarding the whole `output/` directory.

### 2.3 Stage-by-stage notes

- **`probe`** keeps the full keyframe list from ffprobe. The list is load-bearing for `plan`'s keyframe snapping — heuristics aren't sufficient.
- **`detect`** uses `scenedetect`'s Python API directly (no subprocess). Defaults: `AdaptiveDetector`, threshold `27.0`, `min_scene_length=0.6s`. Only one algorithm in v1; the field is in config for future expansion but only `detect-adaptive` is honored.
- **`preview`** generates 3 thumbnails per scene (start / middle / end frames) via `ffmpeg -ss <t> -frames:v 1`. Writes `preview/index.html` with lazy-loaded thumbnails (`<img loading="lazy">`) and an embedded `<video>` element using the `#t=start,end` fragment for click-to-seek. Lazy loading addresses the "thousands of thumbnails per file" scaling concern.
- **`review`** is the gate. Pre-populates `cuts.json` with `keep: <config.review.default_keep>` (default `true`, can be flipped to `false`). Sets status `awaiting_user`. Contract detailed in §4.
- **`plan`** snaps each kept scene's start time to the nearest preceding keyframe (only strategy in v1). Drops scenes with `keep: false`. Applies `merge_with_next`. Writes `cuts-resolved.json` with `requested_*`, `actual_*`, `drift_seconds` per segment.
- **`split`** runs one ffmpeg per segment: `ffmpeg -ss <start> -to <end> -i <src> -map 0 -c copy -avoid_negative_ts make_zero <out>`. Output filename pattern is fixed: `<slug>-highlight-NN[-label].mp4`.
- **`verify`** confirms every planned output exists, is readable by ffprobe, and has duration within tolerance of expected.

### 2.4 What's deliberately absent from v1

- No `detector` field choice (only adaptive).
- No `strategy` choice (only stream-copy).
- No `output_template` config (filename pattern is fixed; future config knob).
- No re-encode block in config.
- No custom-stage registration.

---

## 3. State and data model

Three JSON files own all state. YAML for user-edited config; JSON for machine-managed state.

### 3.1 `manifest.json` (per workspace)

Stage lifecycle fields (`status`, timestamps) are separated from stage artifact fields (counts, configs). Stage code never touches lifecycle keys; orchestrator never touches artifact keys.

```jsonc
{
  "schema_version": 1,
  "id": "birthday",
  "created_at": "2026-05-21T10:14:22Z",
  "updated_at": "2026-05-21T11:02:08Z",

  "source": {
    "relative_path": "Birthday.mov",
    "size_bytes": 1428193024,
    "mtime": "2026-04-15T14:22:18Z",
    "partial_hash": "sha256:a3f4...c89e"
  },

  "metadata": {
    "duration_seconds": 312.521,
    "container": "mov",
    "video_codec": "h264",
    "audio_codec": "aac",
    "fps": 29.97,
    "width": 1920,
    "height": 1080,
    "keyframe_count": 41
  },

  "tool_versions": {
    "vsplit": "0.1.0",
    "ffmpeg": "6.1.1",
    "ffprobe": "6.1.1",
    "scenedetect": "0.6.4"
  },

  "stages": {
    "probe":   { "status": "done", "started_at": "...", "completed_at": "...", "artifacts": {} },
    "detect":  { "status": "done", "started_at": "...", "completed_at": "...",
                 "artifacts": { "scenes_found": 14, "detector": "adaptive" } },
    "preview": { "status": "done", "started_at": "...", "completed_at": "...",
                 "artifacts": { "images_generated": 42 } },
    "review":  { "status": "awaiting_user", "set_at": "...",
                 "artifacts": {
                   "cuts_file_hash": "sha256:b8c1...",
                   "scenes_total": 14,
                   "scenes_marked_keep_initial": 14
                 } },
    "plan":    { "status": "pending", "artifacts": {} },
    "split":   { "status": "pending", "artifacts": {} },
    "verify":  { "status": "pending", "artifacts": {} }
  },

  "config_snapshot": {
    "detect":  { "algorithm": "detect-adaptive", "threshold": 27.0 },
    "split":   { "strategy": "stream_copy" },
    "review":  { "default_keep": true }
  }
}
```

Stage statuses: `pending | running | done | failed | awaiting_user | skipped`. On `failed`, the stage block also gets an `error: { message, exit_code, log_path }` sibling to `artifacts`.

### 3.2 `index.json` (per working folder)

```jsonc
{
  "schema_version": 1,
  "working_root": ".",
  "workspaces": {
    "birthday": {
      "source_relative_path": "Birthday.mov",
      "size_bytes": 1428193024,
      "partial_hash": "sha256:a3f4...c89e",
      "adopted_at": "2026-05-21T10:14:22Z"
    }
  },
  "retired_slugs": ["beach-day-2024", "hike"]
}
```

**`retired_slugs` lifecycle:** an entry is added when `vsplit clean <slug>` removes a workspace. It is cleared from the array the next time a `scan` adopts a source that would produce the same slug, or never (it's only used during slug generation to avoid surprise collisions with recently-removed names).

### 3.3 `decisions/cuts.json` (per workspace, user-edited)

```jsonc
{
  "schema_version": 1,
  "scenes": [
    {
      "id": 1,
      "start": "00:00:00.000",
      "end":   "00:02:34.500",
      "keep": true,
      "label": null,
      "merge_with_next": false
    },
    {
      "id": 2,
      "start": "00:02:34.500",
      "end":   "00:05:12.000",
      "keep": false
    }
  ]
}
```

Validated with `pydantic`. Missing fields default; unknown fields rejected with a clear error pointing to the offending key. No `split_strategy` or `output_template` overrides in v1 — those live in config.

### 3.4 `config.yaml`

```yaml
detect:
  threshold: 27.0
  min_scene_length: 0.6
  luma_only: false

preview:
  images_per_scene: 3
  image_width: 480
  image_quality: 85

review:
  default_keep: true            # set to false for curation workflow
  auto_apply_template: true     # use working/cuts.json.template if present

plan:
  keyframe_snap: nearest_preceding
  min_segment_length: 1.0

split:
  output_container: mp4

verify:
  duration_tolerance_seconds: 0.5

concurrency:
  probe: 4
  detect: 2
  preview: 4
  plan: 8
  split: 1                      # stream-copy is I/O-bound; default to serial
  verify: 4

scan:
  extensions: [mp4, mkv, mov, avi, m4v, webm]
  recursive: false
```

**Resolution order:** built-in defaults → `~/.config/vsplit/config.yaml` → `.vsplit/config.yaml` → CLI flags.

---

## 4. Review gate semantics

The under-specified area of the prior design. Made explicit here.

### 4.1 Three ways to advance past the gate

A workspace at `review.status == "awaiting_user"` advances on the next `vsplit run` when **any** of these is true:

1. **Hash changed.** `sha256(decisions/cuts.json)` differs from `manifest.stages.review.artifacts.cuts_file_hash`. → status becomes `done`.
2. **Explicit approval.** `vsplit approve <slug>` was run since the gate was set. → status becomes `done`.
3. **Template auto-apply.** A `working/cuts.json.template` exists, `review.auto_apply_template` is true, and the workspace hasn't been approved yet. → template is copied to `decisions/cuts.json`, hash recorded, status becomes `done`.

Mtime is **never** used as a decision signal. Mtime is fragile (vim `:wq` with no changes bumps it; file copies reset it; CRLF-aware tools rewrite it). Content hash is unambiguous.

### 4.2 What the gate writes when it first fires

```jsonc
"review": {
  "status": "awaiting_user",
  "set_at": "2026-05-21T10:23:45Z",
  "artifacts": {
    "cuts_file_hash": "sha256:<hash of the file we just wrote>",
    "scenes_total": 14,
    "scenes_marked_keep_initial": 14
  }
}
```

### 4.3 Decision logic on each subsequent `vsplit run`

```
read decisions/cuts.json
current_hash = sha256(file)
recorded_hash = manifest.stages.review.artifacts.cuts_file_hash
was_approved = manifest.stages.review.artifacts.approved_at is set

if file is missing:
    fail clearly ("review file deleted; run `vsplit reset <slug> --from review`")
elif file is invalid JSON or schema-invalid:
    fail with a precise pointer (line, key, expected type)
elif current_hash == recorded_hash AND not was_approved:
    status stays awaiting_user → skip workspace, continue dispatcher
else:
    validate every scene ID in detect/scenes.json appears in cuts.json
        (missing rows → fail with "use keep:false to drop a scene; don't remove the row")
    validate no extra scene IDs in cuts.json
        (extras → fail with "add new scenes by editing detect/scenes.json then `reset --from detect`")
    status = done
    artifacts.cuts_file_hash = current_hash
    artifacts.scenes_marked_keep_final = count of keep:true
    fall through to plan
```

### 4.4 `vsplit approve`

The explicit gate-passage command. Sets `manifest.stages.review.artifacts.approved_at = <timestamp>`; the next run's decision logic advances the gate.

```
vsplit approve birthday
vsplit approve --filter status=awaiting_user
```

This exists because a user may want to accept the pre-populated cuts as-is without editing the file — typical for `default_keep: true` over a clean detection run.

### 4.5 Re-opening the gate

`vsplit reset <slug> --from review` deletes `decisions/`, `plan/`, `output/` and re-pre-populates from `detect/scenes.json`. The user explicitly opts back into the gate; no auto-reverting on detected drift.

### 4.6 Race conditions

The user editing `cuts.json` mid-run is the only race. The dispatcher reads the file once per workspace per scheduling tick; partial saves are handled by validation rejecting the file (invalid JSON or bad schema → fail-loud, no advance). Vim and most editors save atomically via write-then-rename, avoiding this entirely.

### 4.7 Why no file-watch in v1

Watch mode is v2. v1 stays request/response: user edits at their own pace, runs `vsplit run` when ready. A future watcher can be a separate process reading the same files; no core changes needed.

---

## 5. Dispatcher and concurrency

### 5.1 Per-workspace state

```
NEW              workspace just adopted, never run
RUNNING <stage>  a stage is executing
AWAITING_USER    review gate set; user action needed
DONE             all pipeline stages complete
FAILED <stage>   a stage failed; workspace blocked
```

Stored implicitly as the union of `manifest.stages[*].status` values. The dispatcher derives workspace state from those; no separate "workspace state" key is persisted (single source of truth).

### 5.2 Dispatcher loop (pseudocode)

```python
def dispatch(workspaces, config, cancel_event):
    limiters = {stage: Semaphore(config.concurrency[stage]) for stage in STAGE_ORDER}
    inflight: dict[str, Future] = {}

    while not cancel_event.is_set():
        # 1. Reap completed work
        for slug, future in list(inflight.items()):
            if future.done():
                future.result()  # raises if the worker raised
                del inflight[slug]

        # 2. For each idle workspace, find its next runnable stage
        scheduled_this_tick = False
        for ws in workspaces:
            if ws.slug in inflight:
                continue
            next_stage = find_next_runnable_stage(ws.manifest)
            if next_stage is None:
                continue                          # DONE, FAILED, or AWAITING_USER
            limiter = limiters[next_stage]
            if not limiter.try_acquire(blocking=False):
                continue                          # stage at capacity; try next workspace
            future = executor.submit(run_stage, ws, next_stage, limiter)
            inflight[ws.slug] = future
            scheduled_this_tick = True

        # 3. Termination
        if not inflight and not scheduled_this_tick:
            break                                 # nothing to do; exit cleanly

        # 4. Backpressure: wait for at least one inflight to finish
        wait_for_any(inflight.values(), timeout=0.5)
```

**Properties:**

- **Workspace serialization:** a workspace has at most one stage running at a time.
- **Stage parallelism:** different workspaces' stages run in parallel up to per-stage limits.
- **No head-of-line blocking:** if workspace A's `detect` is at capacity, workspace B can still start `preview` if its turn has come.
- **Clean shutdown:** SIGINT sets `cancel_event`; inflight stages see it via `StageContext.cancel_event` and propagate to subprocesses; the dispatcher stops scheduling new work and exits after current work finishes.

### 5.3 `find_next_runnable_stage`

```python
def find_next_runnable_stage(manifest):
    for stage in STAGE_ORDER:                     # fixed order: probe, detect, ..., verify
        s = manifest.stages[stage]
        if s.status == "done":
            continue
        if s.status in ("failed", "awaiting_user"):
            return None                           # workspace blocked
        if s.status == "running":
            return None                           # already in flight (recovery case)
        if s.status == "pending":
            if all_deps_done(stage, manifest):
                return stage
            return None                           # waiting on a dep
        if s.status == "skipped":
            continue
    return None                                   # all done
```

### 5.4 Concurrency defaults

| Stage   | Default | Rationale                                                 |
|---------|---------|-----------------------------------------------------------|
| probe   | 4       | ffprobe is short, I/O light                               |
| detect  | 2       | CPU-heavy, mostly single-threaded per video               |
| preview | 4       | ffmpeg frame extraction parallelizes fine                 |
| plan    | 8       | pure computation, cheap                                   |
| split   | 1       | stream-copy is I/O-bound; parallel ffmpeg on HDD thrashes |
| verify  | 4       | ffprobe again                                             |

User can override per-stage via CLI (`--concurrency split=4` for SSD users) or config.

### 5.5 Failure isolation

A raised exception in `run_stage`:

1. Worker writes `manifest.stages[stage].status = "failed"` and `error = {message, exit_code, log_path}`, persists the manifest.
2. Worker returns control; the future resolves cleanly (the error is in the manifest, not re-raised in the dispatcher).
3. Dispatcher continues scheduling other workspaces' next stages normally.
4. The failed workspace's `find_next_runnable_stage` returns `None` until `vsplit reset` clears the failure.

This is the property "one broken file doesn't block the others."

### 5.6 Cancellation and crash recovery

**Cancellation (Ctrl-C):**

- Signal handler sets `cancel_event`.
- Workers check the event between expensive steps; subprocess calls receive `terminate()` then `kill()` after a 5-second grace.
- The worker that was running a stage at cancel time leaves `manifest.stages[stage].status = "running"` as-is (best-effort: it may not get a chance to update it). Partial outputs may exist on disk.
- Dispatcher stops scheduling new work and exits with code 130 after inflight workers return.

**Startup recovery (next `vsplit run`):**

- Before the dispatcher loop, the orchestrator scans manifests for any stage with `status == "running"`.
- For each such stage, the orchestrator invokes a stage-specific `recover(ctx)` method (default implementation: delete partial outputs declared in `produces`, reset status to `pending`).
- Stages that need custom recovery (notably `split`, which may have partially-written `output/*.mp4` files) override `recover()` to validate and either keep or delete partial outputs.
- After recovery, normal dispatch resumes.

This keeps the dispatcher loop simple (it never sees `running` workspaces from a previous process) and gives stages a chance to be smart about partial state when needed.

### 5.7 Single process, threads not processes

Everything in one Python process. Subprocesses only for ffmpeg/ffprobe. No `multiprocessing` — PySceneDetect releases the GIL during analysis, so threads suffice. No daemon, no IPC, no shared state. Restart is free; all state is on disk.

---

## 6. CLI surface

Sharp v1: six commands plus a `one` shortcut.

### 6.1 Global flags

```
--working-dir, -w PATH        # default: cwd
--config PATH                 # override config location
--log-level {debug,info,warn,error}   # default: info
--json                        # machine-readable output where applicable
```

### 6.2 Commands

#### `vsplit scan [path]`

Discover videos and adopt new ones. Idempotent.

```
vsplit scan                          # current dir, non-recursive
vsplit scan ./working --recursive
vsplit scan --dry-run                # print what would be adopted
```

Adoption per file:

1. Compute size + mtime + partial hash.
2. Look up in index by hash, then by relative path.
3. New → slugify, check collision against live workspaces + `retired_slugs`, create workspace, register in index.
4. Hash matches but path differs → prompt `rename detected — update pointer? (y/N)`, or skip prompt under `--auto-relocate`.
5. Path matches but hash differs → warn `source replaced; existing workspace state may be stale` (no auto-overwrite).

#### `vsplit run [filter]`

Advance workspaces through the pipeline.

```
vsplit run                                   # everything as far as possible
vsplit run birthday                          # one slug
vsplit run "vacation-*"                      # glob
vsplit run --filter status=awaiting_user
vsplit run --filter status=failed
vsplit run --through preview                 # stop after preview
vsplit run --from plan                       # start from plan
vsplit run --only detect                     # just this stage
vsplit run --concurrency split=4             # per-stage override
vsplit run --dry-run
```

**Filter grammar (locked for v1):**

- `--filter key=value` for predicates: `status`, `stage`, `slug`.
- `status` values: any of `pending | running | done | failed | awaiting_user | skipped`.
- `stage` values: any stage name; matches workspaces where that stage is the next runnable one.
- `slug` value is a fnmatch glob (e.g. `slug=vacation-*`).
- Repeating `--filter` ANDs predicates.
- The bare positional argument is shorthand for `--filter slug=<value>` (still glob-evaluated).
- No OR, no nesting, no negation. Deferred to v2.
- Applies identically to `run`, `status`, `approve`, `reset`, `clean`.

#### `vsplit status [filter]`

```
vsplit status                                # all workspaces, table
vsplit status birthday                       # detailed one
vsplit status --awaiting-user
vsplit status --failed
vsplit status --json
```

Default table format:

```
SLUG               PROBE  DETECT  PREVIEW  REVIEW  PLAN  SPLIT  VERIFY
birthday            ok     ok      ok       wait    -     -      -
beach-day           ok     ok      ok       ok      ok    ok     ok
hike                ok     FAIL    -        -       -     -      -

3 workspaces · 1 done · 1 awaiting user · 1 failed
```

#### `vsplit approve <filter>`

The explicit gate-passage command from §4.

```
vsplit approve birthday
vsplit approve --filter status=awaiting_user
```

#### `vsplit reset <filter> [--from <stage>]`

```
vsplit reset birthday --from detect          # re-run detect onward
vsplit reset birthday --all                  # full reset
vsplit reset --filter status=failed --from detect
```

Reset deletes artifacts produced by reset stages and clears their manifest entries. Source files never touched. Confirmation prompt for `--all` unless `--force`.

#### `vsplit clean <filter>`

Delete workspaces entirely.

```
vsplit clean birthday
vsplit clean --all
vsplit clean --filter status=done
```

Adds the slug to `retired_slugs`. Confirmation prompt unless `--force`.

### 6.3 Convenience: `vsplit one <file>`

```
vsplit one ./Birthday.mov
vsplit one ./Birthday.mov --through preview
vsplit one ./Birthday.mov --auto-approve     # skip the gate; useful for templates
```

Implicitly: `scan` the parent directory (only that file), `run` through `preview` (or `--through`), set status to `awaiting_user`. Opens the preview HTML in the default browser if `webbrowser.open()` is available. After the user edits `cuts.json` and runs `vsplit run`, completes split.

Single-file workflow gets first-class treatment so "I just want to trim this one clip" isn't five commands.

### 6.4 Deliberately absent in v1

- `vsplit relocate` — covered by `scan --auto-relocate`.
- `vsplit doctor` — replaced by good error messages from each command on first failure.
- `vsplit config show/edit/path` — config is one file; use any editor.
- `vsplit watch` — v2.
- Plug-in / custom stage management — no plug-ins in v1.

---

## 7. Filesystem layout, project structure, tech stack

### 7.1 Working folder layout

```
working/                                  # user's chosen folder
  Birthday.mov                            # source files, untouched
  Beach Day.mp4
  Hike.mkv
  vsplit.config.yaml                      # optional project-local override
  cuts.json.template                      # optional, for batch templating
  .vsplit/
    index.json
    config.yaml                           # effective resolved config (cache)
    workspaces/
      birthday/
        manifest.json
        probe/
          ffprobe.json
          keyframes.csv
        detect/
          scenes.json
        preview/
          images/
            scene-001-01.jpg
            scene-001-02.jpg
            scene-001-03.jpg
            ...
          index.html
        decisions/
          cuts.json                       # USER EDITS THIS
        plan/
          cuts-resolved.json
        output/
          birthday-highlight-01.mp4
          birthday-highlight-02-cake.mp4
        logs/
          probe.log  detect.log  preview.log
          plan.log   split.log   verify.log
      beach-day/
        ...
```

**Removed from prior design:** `detect/scenes.csv` (JSON is canonical; CSV not consumed downstream), `preview/scenes.md` (HTML is the review surface; markdown duplicates without benefit).

### 7.2 Project layout

```
vsplit/
  pyproject.toml                          # uv-managed
  README.md
  src/vsplit/
    __init__.py
    __main__.py                           # `python -m vsplit`
    cli/
      __init__.py
      scan.py
      run.py
      status.py
      approve.py
      reset.py
      clean.py
      one.py
      _filter.py                          # filter grammar parser
      _output.py                          # table + JSON renderers
    core/
      __init__.py
      orchestrator.py                     # dispatcher loop (§5)
      pipeline.py                         # per-workspace stage runner
      manifest.py                         # read/write, schema migration
      index.py
      slugify.py                          # Windows-safe slug gen
      paths.py                            # cross-platform helpers
      hash.py                             # partial + content hashing
      atomic.py                           # write-then-rename helper
      types.py                            # Stage protocol, Manifest, Config
    stages/
      __init__.py
      probe.py
      detect.py
      preview.py
      review.py
      plan.py
      split.py
      verify.py
    schemas/
      manifest.py                         # pydantic models
      config.py
      cuts.py
      index.py
    templates/
      preview_index.html.j2               # Jinja2 template
  tests/
    unit/
      test_slugify.py
      test_manifest.py
      test_filter.py
      test_plan.py
      ...
    integration/
      fixtures/                           # tiny 5-sec test videos
      test_pipeline_end_to_end.py
      test_failure_isolation.py
      test_review_gate.py
      test_resume.py
```

Module sizing target: every file under ~300 lines. The seven pipeline stages live in seven files, not one.

### 7.3 Runtime requirements

- Python ≥ 3.11 (for `tomllib`, `Self`, better error messages)
- FFmpeg + FFprobe ≥ 5.0 on PATH
- PySceneDetect ≥ 0.6.4 (as a Python package; no separate CLI install)

### 7.4 Python packages

| Package                | Purpose                                                |
|------------------------|--------------------------------------------------------|
| `click`                | CLI framework                                          |
| `pydantic` ≥ 2         | Schema validation for manifest, config, cuts.json      |
| `pyyaml`               | Config file parsing                                    |
| `scenedetect[opencv]`  | Scene detection, used as a library                     |
| `jinja2`               | Preview HTML template                                  |
| `rich`                 | Status table + colored output (auto-detects TTY)       |
| `structlog`            | Structured logging to per-stage log files              |
| `platformdirs`         | Cross-platform config/cache locations                  |

**Notable choices and rejected alternatives:**

- **`click` over `typer`/`argparse`/`fire`:** click is the most boring choice. Typer's type-magic surprises; argparse boilerplate is heavy; fire's dispatch is too clever for a tool used in scripts.
- **`pydantic` v2 over dataclasses + manual validation:** schema errors with line/col pointers are the user-facing surface for `cuts.json` failures. pydantic does this; hand-rolling doesn't.
- **`scenedetect` as a library, not CLI subprocess:** removes the brittle "parse the CSV output" step from the prior design. The Python API is stable.
- **`structlog` over stdlib logging:** per-stage log files plus a future JSON aggregate log are easier with structlog's processors.
- **`uv` for dev:** fast, reproducible, aligns with `pyproject.toml`. End users install via `pipx install vsplit` or `uv tool install vsplit`.

### 7.5 Windows-specific behavior (carried forward, scoped down)

- Slug generation: lowercase, strip Windows-reserved characters (`< > : " / \ | ? *`), strip trailing dots/spaces, prepend `_` if matching a reserved name (`con`, `nul`, `aux`, `com1`–`com9`, `lpt1`–`lpt9`), truncate to 64, append `-2/-3/...` on collision (case-insensitive comparison against live + `retired_slugs`).
- All subprocess calls use argument lists, never shell strings (`subprocess.run([...], shell=False)`).
- Source paths stored relative; resolved at runtime with `pathlib.Path.resolve()`.
- Path length: warn at scan time if the projected `<root>\.vsplit\workspaces\<slug>\preview\images\scene-XXX-XX.jpg` exceeds 240 chars. No long-path-prefix magic.
- Text files written with LF line endings.

**Removed from prior design:** console code-page handling (Python 3.11 + Windows Terminal handles UTF-8 fine by default; no `chcp 65001`), anti-virus workaround logic (documented in README, not coded).

### 7.6 Testing approach

- **Unit tests:** slugify, filter parser, manifest read/write/migrate, hash computation, plan stage's keyframe-snap math. Pure functions; fast.
- **Integration tests:** end-to-end pipeline against checked-in 5-second test videos (one mp4, one mkv, one with VFR). Covers happy path, resume-after-kill, review-gate hash semantics, failure isolation (corrupted file mixed into a batch).
- **No subprocess mocking:** real ffmpeg/scenedetect are required for integration tests; CI installs both. The contract with these tools IS the test surface.

---

## 8. Open questions for implementation

- **Slug for `vsplit one <file>` when the file is outside the working folder:** do we treat the file's parent directory as a one-off working folder, or require the user to `cd` first? Lean: treat parent as working folder.
- **Manifest schema migration policy:** v1 has schema 1, so no migration yet. Document the policy now (auto-migrate with backup file) so v2 isn't a fire-drill.
- **Output filename collisions across workspaces:** `birthday-highlight-01.mp4` and `beach-day-highlight-01.mp4` are distinct; no issue. But if the user ever copies all `output/` contents to one folder, collisions are possible. Documented in README; not solved in v1.
- **HTML preview when the working folder is on a network share:** browsers may refuse `file://` access to UNC paths. Documented; not solved in v1.

## 9. Glossary

- **Working folder:** the user's chosen directory containing source video files and the `.vsplit/` state subdirectory.
- **Workspace:** one source file's state, living at `.vsplit/workspaces/<slug>/`.
- **Slug:** short, filesystem-safe identifier derived from the source filename; the workspace directory name.
- **Manifest:** per-workspace JSON file tracking source identity, metadata, stage progress, and tool versions.
- **Stage:** one step in the per-file pipeline (`probe`, `detect`, `preview`, `review`, `plan`, `split`, `verify`).
- **Pipeline:** the fixed ordered sequence of stages run per workspace.
- **Orchestrator:** the component that scans for files, dispatches per-workspace pipelines, manages concurrency, isolates failures.
- **Stream-copy:** lossless splitting via `ffmpeg -c copy`. Cuts snap to keyframes.
- **Partial hash:** `sha256` of the first 1 MB + last 1 MB + size of a file. Used for source identity without reading entire files.
- **Adoption:** the process by which `vsplit scan` registers a source file into the index and creates its workspace.
