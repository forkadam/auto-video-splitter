# vsplit — Design Doc

> Companion to `vsplit-implementation-spec.md`. The spec describes **what** is being built; this doc describes **why** the design looks the way it does, what alternatives were considered, and what tradeoffs were accepted.
>
> Status: draft v0.1.

---

## 1. Background and motivation

The user has a recurring task: take a video file (or a folder of 50+ video files), automatically detect scene boundaries, review the detected scenes, and split each video into separate clips at the right boundaries. The cuts need to be lossless or near-lossless. The entire workflow needs to run locally — no cloud upload, no SaaS.

The existing landscape:

- **GUI tools** (LosslessCut, Avidemux, Shotcut, DaVinci Resolve) handle individual files well but are not designed for batch operation. Scripting them is awkward to impossible.
- **Command-line tools** in isolation (PySceneDetect for detection, FFmpeg for cutting) work but require manual orchestration: scripting the handoff between detection output and cutting input, organizing artifacts, managing state across runs, handling failures, supporting human review.
- **No tool we're aware of** combines automated scene detection, batch orchestration over 50+ files, a human review gate, and lossless splitting in a single workflow.

The gap is in the **orchestration**, not the underlying primitives. PySceneDetect is excellent at detection; FFmpeg is excellent at cutting. What's missing is the connective tissue: a stateful, resumable, scriptable pipeline that drives both, accommodates a human review step, and scales from one file to many.

## 2. Problem statement

Build a local CLI tool that:

1. Discovers video files in a working folder and tracks them as managed workspaces.
2. Runs automated scene detection per video.
3. Generates browsable previews so the user can review detected scenes without a GUI.
4. Lets the user adjust the cut list by editing plain JSON.
5. Executes lossless splits via FFmpeg.
6. Survives interruption — every stage is resumable.
7. Isolates failures — one broken video doesn't block the other 49.
8. Allows extension via plug-in stages without modifying the core.

## 3. Constraints

These were imposed by context rather than design choice and shape multiple decisions downstream:

- **Windows-first.** Most other constraints follow from this. No symlinks, no shell-string commands, careful path length handling, case-insensitive filesystem assumptions.
- **Local-only.** No outbound network in the hot path. Acceptable for a model download during install, not during pipeline execution.
- **No GUI.** The user's workflow is terminal-centric (vim, command line). HTML preview pages are acceptable static artifacts opened in a browser; a desktop application is not.
- **Single-developer initially.** Design should be understandable and modifiable by one person. No microservices, no message queues, no databases-with-schemas-to-migrate.
- **Source files are precious.** Never modified, never moved, never overwritten. Read-only inputs.

## 4. Design overview

Two layers, related by composition:

The **orchestrator** is the only component aware of "there are multiple videos." It scans the working folder, maintains an index of adopted sources, and dispatches work to per-file pipelines with concurrency limits.

The **per-file pipeline** is unaware of batch context. It takes one workspace and advances it through ordered stages. Each stage is a self-contained module that reads from and writes to the workspace folder, persisting state in a manifest file.

Critically, **the filesystem is the integration layer**. Stages don't pass data via function calls or shared memory; they pass it via files. This is the central design choice from which most others follow.

```
+------------------------------------------------------------+
|  Orchestrator                                              |
|  - discovery     - dispatch                                |
|  - index         - concurrency limits                      |
|  - status        - failure isolation                       |
+--------+---------------------+----------------+------------+
         |                     |                |
         v                     v                v
   +-----------+         +-----------+    +-----------+
   | Pipeline  |         | Pipeline  |    | Pipeline  |
   | (file 1)  |         | (file 2)  |    | (file N)  |
   +-----------+         +-----------+    +-----------+
         |
         v
   probe -> detect -> preview -> review -> plan -> split -> verify
         (state persisted to workspace/manifest.json at each step)
```

## 5. Key design decisions

Each decision below includes: the choice, the alternatives considered, and the rationale.

### 5.1 Two-layer architecture (orchestrator + per-file pipeline)

**Choice**: Cleanly separate "batch concerns" (orchestration over many files) from "per-file concerns" (the actual scene-detect-then-split workflow).

**Alternatives**:

- *Monolithic single-pass*: one big script that iterates over files and does everything sequentially. Simple to write but conflates concerns: failure handling, resume logic, concurrency control all entangled with detection and cutting logic.
- *DAG over all files*: model the whole batch as a single dependency graph of tasks. More flexible but heavyweight — pulling in a workflow engine (Airflow, Prefect, Make) for what is fundamentally a linear per-file pipeline.

**Rationale**: The two-layer split matches the mental model. "Run detect on every video" is a batch operation; "decide which scenes to keep in this specific video" is per-file. Conflating them forces every stage to know about batching, and forces the orchestrator to know about scene detection. Separation lets each layer be tested and reasoned about independently.

A side benefit: custom stages can be written without thinking about concurrency or batch state at all. The orchestrator handles that.

### 5.2 Manifest file as source of truth (not a database)

**Choice**: One JSON file per workspace tracks all state. No SQLite, no embedded database, no in-memory state.

**Alternatives**:

- *SQLite*: structured queries, transactional updates, single-file portability. Good fit on paper.
- *Single JSON catalog* at working-folder level: one file lists all workspaces and all stages. Centralized.
- *In-memory state with append-only log*: replay log on resume.

**Rationale**:

- Per-workspace manifest files are independently inspectable. Open `manifest.json` in vim, see exactly what's happened. Critical for a tool the user will frequently inspect.
- No schema migrations. Add a field to the manifest, old workspaces still work (missing fields default).
- Git-friendly. The user can version-control a working folder if they want.
- Concurrent writes are simpler: workspace A's manifest and workspace B's manifest are different files, no locking needed.
- Survives crash trivially: write-then-rename gives atomic-ish updates; worst case, one stage's state is lost and gets re-run.
- No additional runtime dependency. SQLite would need bindings, version pinning, file-locking on Windows network drives, etc.

The cost: querying across workspaces requires reading multiple files. Acceptable at 50 workspaces; would be a problem at 5,000. Out of scope.

### 5.3 Filesystem as the integration bus between stages

**Choice**: Stages communicate by reading and writing files in the workspace. No in-memory data passing between stages.

**Alternatives**:

- *In-process pipeline with shared data structures*: faster, no serialization overhead.
- *Message queue between stages*: more decoupled, supports distributed execution.

**Rationale**: Inspectability and debuggability dominate. After a failed run, the user can look at `detect/scenes.json` and see exactly what detection produced. They can re-run just `plan` against existing detection results without re-detecting. They can manually edit any intermediate artifact if needed. Custom stages can be written in any language (not just TypeScript) because the contract is "read these files, write those files," not "implement this TypeScript interface."

The cost: serialization overhead. For a tool dominated by ffmpeg/scenedetect subprocess time, this is in the noise.

### 5.4 PySceneDetect + FFmpeg over alternatives

**Choice**: Shell out to PySceneDetect for scene detection and FFmpeg for cutting.

**Alternatives considered**:

- *TransNetV2*: deep-learning shot detector with better accuracy on dissolves and gradual transitions. Pulls in PyTorch (~2GB), GPU optional.
- *OpenCV from scratch*: implement detection directly. Reinventing the wheel.
- *LosslessCut headless*: it's a real, working tool. But it's Electron-based, not designed for headless batch operation, and its API isn't stable.
- *FFmpeg's `scenedetect` filter*: built into FFmpeg as `scdet`. Less configurable, no Python ecosystem.

**Rationale**: PySceneDetect is mature, configurable (multiple detection algorithms), well-documented, and has a clean CLI. It's already what most people use for this. TransNetV2 is better for tough content but the dependency cost is high; it lives in the future-work list as an optional alternative `detect` stage. FFmpeg is the universal solvent for video cutting and exists on every platform.

Shelling out (rather than embedding) means we get bug fixes and improvements upstream for free, and we don't need Python bindings in our Node process.

### 5.5 Stream-copy by default, re-encode opt-in

**Choice**: Default split strategy is stream-copy (`ffmpeg -c copy`), with re-encode and smart-cut as opt-in alternatives.

**Alternatives**:

- *Re-encode by default* (e.g., `-crf 17 -preset slow`): frame-accurate cuts but slower and lossy (visually lossless ≠ mathematically lossless unless `-crf 0`).
- *Smart-cut by default*: re-encode only the GOP straddling each cut, stream-copy the rest. Best of both worlds, but complex to implement correctly.

**Rationale**: The user's stated requirement is lossless. Stream-copy is the only mathematically lossless option. The tradeoff is that cuts snap to keyframes (off by up to a GOP, typically 2–10 seconds). For most scene-based splitting, the detected scene boundary is close enough to a keyframe that this isn't a problem — and where it is, the user can switch to re-encode for that workspace by editing `cuts.json`.

Smart-cut is the right long-term answer but is complex enough to defer. The architecture supports adding it as a new `split_strategy` value without breaking anything.

### 5.6 Source files referenced by relative path, never modified or linked

**Choice**: Source files stay where the user put them. The manifest stores a path relative to the working-folder root. No symlinks, hardlinks, or copies into the workspace.

**Alternatives**:

- *Symlink into workspace*: nice mental model ("everything for this video is in one folder") but requires admin on Windows pre-Dev-Mode.
- *Hardlink into workspace*: works without admin but only within the same NTFS volume. Cross-drive setups break.
- *Copy source into workspace*: simplest but doubles disk usage. With 50 large videos, this is hundreds of GB of waste.
- *Move source into workspace*: makes the source disappear from the user's view, which is unacceptable.

**Rationale**: The relative path is the simplest thing that works everywhere. Manifests stay portable (the whole working folder can be moved to a USB drive, mounted under a different drive letter, etc.). No filesystem privileges required. The minor loss of "see source in workspace folder" is acceptable; the user can always open the working folder.

A future convenience could be generating a `.url` shortcut file (Windows) or `source.txt` containing the path, but it's not on the critical path.

### 5.7 Human-in-the-loop via plain JSON editing

**Choice**: The user reviews detected scenes by opening a generated HTML page in their browser and editing `decisions/cuts.json` in their editor.

**Alternatives**:

- *Built-in web UI*: spin up a local server, present a review interface in the browser.
- *TUI (terminal UI)*: blessed/ink-based interactive review in the terminal.
- *Interactive CLI prompts*: `vsplit review goldeneye` walks the user through each scene with y/n prompts.

**Rationale**: The user is vim-centric and JSON-fluent. Editing a JSON file is faster than clicking through a UI for someone who knows what they want. It's also scriptable: the user can write a script that generates `cuts.json` files from external metadata (chapter markers, EDL files, manual notes). And it's diff-able: changes can be reviewed in git, shared, templated.

The HTML preview page is generated as a static file with no server. Open it in any browser. The `<video>` tag's `#t=start,end` fragment lets the user seek to each scene without any custom JS.

A future TUI for the "rapid review" case (mark every scene keep/drop quickly) is plausible but not in v1.

### 5.8 Plug-in stages via config + module path

**Choice**: Custom stages are TypeScript/JavaScript modules listed in the config file. They implement a single `Stage` interface.

**Alternatives**:

- *Hardcoded stage list*: no plug-in support. Simpler but inflexible.
- *Plugin manifest with metadata*: stages declare capabilities, the orchestrator chooses based on the task. Heavyweight; appropriate for an IDE plugin system, not this.
- *Subprocess-based plugins (shell scripts as stages)*: language-agnostic at the cost of a more complex contract.

**Rationale**: The Node module system already does what we need — load a file by path, get an exported object. The `Stage` interface is small (one method) and the file-based artifact contract means stages can be very thin wrappers around external tools.

Subprocess plugins are a credible alternative and may be added later (a `Stage` that just shells out to an arbitrary script). But the first-class plug-in path being TypeScript matches the user's stack.

### 5.9 Partial hash for source identity

**Choice**: Identify a source file by `size_bytes + mtime + sha256(first 1 MB + last 1 MB + size_bytes)`.

**Alternatives**:

- *Full sha256*: definitive but slow. Hashing 50 × 5 GB files takes minutes.
- *Size + mtime only*: fast but fragile (touch the file, hash changes; copy to a new filesystem, mtime resets).
- *Size only*: too collidable.

**Rationale**: Partial hash is fast (~tens of milliseconds per file) and has effectively zero collision probability for non-adversarial inputs. We're not defending against malicious actors trying to spoof identity; we're trying to detect "this file was replaced" or "this file was renamed." Partial hash handles both.

The full hash could be added as an optional integrity-check stage later.

### 5.10 Single Node process, no workers, no daemon

**Choice**: Everything runs in one Node process. Subprocesses are only for FFmpeg, FFprobe, and scenedetect. No `worker_threads`, no `cluster`, no background daemon.

**Alternatives**:

- *Worker threads* per workspace: parallelism within Node.
- *Daemon* that holds state in memory, CLI clients talk to it via IPC.
- *Per-workspace child processes* of the Node CLI itself.

**Rationale**: The heavy lifting (encoding, detection) happens in subprocess. Node is just orchestration — checking statuses, writing JSON files, dispatching subprocesses. None of that needs parallelism beyond what `p-limit` over `execa` calls already provides.

A daemon would add a class of bugs (stale state, IPC, restart semantics) for no observable benefit. Process restart is free; state is on disk.

### 5.11 YAML for config, JSON for state

**Choice**: User-edited configuration in YAML; machine-managed state (manifest, index, intermediate artifacts) in JSON.

**Alternatives**:

- *JSON for everything*: consistent but worse for human-edited files (no comments, strict syntax).
- *TOML for config*: also human-friendly. YAML chosen for being more widely known.
- *JS module as config*: full programmability. Overkill, and harder to validate.

**Rationale**: YAML is hand-editable with comments, which configs need. JSON is faster to parse and easier for machines to generate without formatting quirks. The split matches the audience: humans write YAML; the program writes JSON.

### 5.12 Per-stage log files, not interleaved

**Choice**: Each stage writes to its own log file (`workspace/logs/<stage>.log`).

**Alternatives**:

- *Single log per workspace*: simpler but everything mixed together.
- *Single log per pipeline run* across all workspaces: shows the full picture but interleaves output from 50 ffmpeg processes.

**Rationale**: When a stage fails, the user wants to see exactly what that stage said. Per-stage log files mean "the detect log for goldeneye" is one obvious file. They're also small (typically <100 KB), so easy to share when reporting bugs. The structured JSON log can be added later as an aggregate (`logs/run-<timestamp>.ndjson`) without removing per-stage files.

## 6. Tradeoffs explicitly accepted

These are losses we knowingly take in exchange for the wins above:

- **Cuts are keyframe-aligned, not frame-accurate, in default stream-copy mode.** Documented; user can switch strategies.
- **No transactional updates across multiple files.** If the process dies mid-write to two related files, one might be updated and the other not. Mitigated by write-then-rename on the manifest and by reading-and-validating before stage runs.
- **No cross-workspace state queries.** Listing "all failed stages across 50 workspaces" requires reading 50 manifest files. Fast enough at this scale; not a fit for 5,000+ workspaces.
- **Custom stages have full Node permissions.** A malicious custom stage could read arbitrary files or make network calls. Out of scope to sandbox in v1; the user controls what's in their `custom_stages` list.
- **Concurrency tuning is manual.** Defaults are reasonable; optimal values depend on hardware. No auto-tuning.
- **PySceneDetect requires Python.** Adds an install step on Windows where Python isn't standard. Documented in setup.
- **No undo.** Once `vsplit clean goldeneye` runs, the workspace is gone. The source file is untouched, but the detection results, previews, and cut decisions are lost. Confirmation prompts mitigate but don't eliminate.

## 7. Risks and mitigations

| Risk                                                              | Likelihood | Impact | Mitigation                                                        |
|-------------------------------------------------------------------|------------|--------|-------------------------------------------------------------------|
| PySceneDetect output format changes across versions               | Low        | Medium | Pin minimum version; parse defensively; integration test on bump  |
| FFmpeg behavior varies across builds (especially Windows zranger) | Medium     | Medium | `vsplit doctor` checks version; document supported builds         |
| Long Windows paths cause silent failures                          | Medium     | High   | `vsplit doctor` flags projected lengths; document `\\?\` prefix   |
| Manifest corrupted by crash mid-write                             | Low        | High   | Write-then-rename; validate on read; recover from `manifest.bak`  |
| User edits `cuts.json` to invalid JSON                            | High       | Low    | `zod` validation with clear error messages; preserve original     |
| Detection misses scenes on unusual content (animation, slideshow) | Medium     | Low    | Documented; alternative detectors via config; future TransNetV2   |
| Disk fills mid-split                                              | Low        | High   | `vsplit doctor` estimates required space; clear error on failure  |
| Source file changes between detection and split                   | Low        | Medium | Re-check hash at split time; warn if changed                      |
| Anti-virus interferes with preview image generation               | Medium     | Low    | Document workaround; tolerate transient EBUSY in stage runner     |

## 8. Alternatives considered (whole-project)

These are entire-project alternatives we rejected before settling on the current design.

### 8.1 Build on top of LosslessCut

LosslessCut is an Electron-based GUI that does most of what we need for a single file. We considered automating it (CLI flags, file-based input).

**Rejected because**: it's not designed for batch operation, its CLI surface is limited, automating an Electron app for headless use is fragile, and pulling in Electron as a dependency violates the "no GUI" constraint.

LosslessCut remains a great reference implementation for smart-cut (their `mergedSplit` algorithm) and we may study its code when implementing the smart-cut strategy.

### 8.2 Build on top of Auto-Editor

Auto-Editor is a Python tool that automatically cuts videos based on audio silence and motion detection.

**Rejected because**: it's solving a different problem (removing silence/inactive segments, primarily for podcast/lecture content). It doesn't do scene detection in the traditional sense and isn't designed for the human-review workflow we need.

### 8.3 Write the whole thing in Python

Python is the natural language for PySceneDetect integration and has good FFmpeg bindings.

**Rejected because**: the user's stack is TypeScript/Node. Maintainability and modifiability by the user (who will be the primary developer for now) outweigh the integration ergonomics. Shelling out to PySceneDetect from Node works fine; the overhead is negligible compared to the actual detection time.

### 8.4 Write a Makefile-based pipeline

A Makefile with one target per stage would give us resume semantics for free (Make only rebuilds out-of-date targets).

**Rejected because**: Make is awkward on Windows (requires WSL, Git Bash, or Cygwin), its dependency tracking by mtime doesn't compose well with our content-hash identity model, and per-workspace state would require generating per-workspace Makefiles. Conceptually elegant but operationally painful.

Worth noting: the conceptual model (stages with explicit inputs/outputs, re-run only what's stale) is borrowed from Make even though the implementation isn't.

### 8.5 Embed PySceneDetect via a Python bridge

Use `python-shell` or similar to run PySceneDetect in-process from Node, avoiding subprocess startup overhead.

**Rejected because**: startup overhead is irrelevant when detection takes minutes per video. The subprocess boundary gives us isolation (a PySceneDetect crash doesn't kill our Node process) and lets us shell out to a `scenedetect` binary installed however the user wants.

## 9. Open design questions

These remain unresolved at v0.1 and will be revisited during implementation:

- **Manifest schema versioning policy.** When the schema changes, do we auto-migrate, refuse to load, or migrate-on-write? Lean toward auto-migrate with a backup file.
- **Behavior when source disappears.** If the user deletes a source file but its workspace exists, what should `vsplit run` do? Skip? Fail? Offer to clean up?
- **Atomic batch operations.** Should `vsplit clean --all` be atomic (all or none) or best-effort? Lean best-effort.
- **Output naming collisions.** Two videos in the same working folder both produce `scene-001.mp4` outputs in their respective workspaces. Fine in isolation but bad if the user copies all outputs to a single folder. Should the default filename include the workspace slug?
- **Custom stages distribution.** Should we support installing custom stages via npm (`npm install vsplit-stage-whisper`)? Or only local file paths? Probably both; npm adds discovery (a registry of community stages).

## 10. Glossary

- **Working folder**: the user's chosen directory containing source video files and the `.vsplit/` state subdirectory.
- **Workspace**: one managed source file's state, living at `.vsplit/workspaces/<slug>/`.
- **Slug**: short, filesystem-safe identifier derived from the source filename; the workspace directory name.
- **Manifest**: per-workspace JSON file tracking state, metadata, and stage progress.
- **Stage**: one step in the per-file pipeline (probe, detect, preview, review, plan, split, verify). Implements the `Stage` interface.
- **Pipeline**: the ordered list of stages run per workspace. Configurable.
- **Orchestrator**: the component that scans for files, dispatches per-workspace pipelines, manages concurrency, isolates failures.
- **Stream-copy**: lossless splitting via `ffmpeg -c copy`. Cuts snap to keyframes.
- **Smart-cut**: hybrid strategy that re-encodes only the GOP straddling each cut. Frame-accurate, near-lossless, complex. Future work.
- **Adoption**: process by which `vsplit scan` registers a source file into the index and creates its workspace.
- **GOP (Group of Pictures)**: the span of frames between two keyframes in a compressed video stream. Stream-copy cuts can only land on GOP boundaries.

## 11. References

- PySceneDetect documentation: https://www.scenedetect.com/
- FFmpeg stream-copy and cutting: https://trac.ffmpeg.org/wiki/Seeking
- TransNetV2 (future detector): https://github.com/soCzech/TransNetV2
- LosslessCut (smart-cut reference): https://github.com/mifi/lossless-cut
- Implementation specification: `vsplit-implementation-spec.md`
