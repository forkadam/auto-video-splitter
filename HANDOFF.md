# vsplit — Session Handoff

> Pick up where the previous session left off. Read this first, then `docs/superpowers/plans/2026-05-21-vsplit-v1.md` Task 1.

**Repo:** `/Users/adamcheng/Dev/Workspace/auto-video-spliter` (Mac path; adjust on your machine)
**Branch:** `master` · **Default PR base:** `main`
**Last commit:** `10407da` — implementation plan
**Date of handoff:** 2026-05-21

---

## Current goal

Build **vsplit v1** — a Python CLI that scans a folder of personal video clips, detects scene boundaries with PySceneDetect, gates on a human review step, and losslessly splits each file via FFmpeg. Per-file workspaces under `.vsplit/`, manifest-driven, resumable, failure-isolated.

The driving use case is **vacation/event footage** (50–200 short clips per trip on Windows), not movies. Default behavior is curation-friendly: keep few, drop many.

## What's done

| What | Where | Commit |
|---|---|---|
| Brainstormed redesign from first principles using `superpowers:brainstorming` | conversation | — |
| Wrote final design spec (10 sections, 794 lines) | `docs/superpowers/specs/2026-05-21-vsplit-design.md` | `39add96` |
| Wrote implementation plan (36 tasks, 9 phases, ~5200 lines) | `docs/superpowers/plans/2026-05-21-vsplit-v1.md` | `10407da` |
| Self-reviewed both docs, fixed gaps inline | — | (included in commits above) |

**No code has been written yet.** The repo currently contains only documentation.

## What's in progress

User was asked to choose execution method for the plan:
1. **Subagent-driven** (recommended) — fresh subagent per task via `superpowers:subagent-driven-development`
2. **Inline** — execute in current session via `superpowers:executing-plans`

Awaiting their choice. Once chosen, execution starts at **Task 1** of the plan.

## Files in the repo

**Committed:**
- `docs/superpowers/specs/2026-05-21-vsplit-design.md` — the authoritative spec
- `docs/superpowers/plans/2026-05-21-vsplit-v1.md` — the TDD task plan

**Untracked (intentional):**
- `docs/vsplit-design-doc.md`, `docs/vsplit-implementation-spec.md` — the user's *original* design docs from before the redesign. Kept for historical reference; the new spec supersedes them. Leave them untracked unless the user asks otherwise.
- `.claude/` — local Claude settings; do not commit.
- `.DS_Store` — macOS turd; do not commit.

## Key decisions (and why)

These are the load-bearing decisions from brainstorming. Don't relitigate without checking back.

| Decision | Why |
|---|---|
| **Python (not TypeScript)** | Direct PySceneDetect API (no subprocess), simpler Windows install. User OK'd it but flagged Bun preference if/when a UI is added later. |
| **Per-file workspaces** | User confirmed this over a "single batch project" model. Each video gets its own `.vsplit/workspaces/<slug>/` folder. |
| **Stream-copy default** | User confirmed lossless over frame-accurate. Cuts snap to keyframes (off by up to a GOP, typically 1–2s on phone footage). |
| **Sharp v1 scope** | Cut: plug-in stages, custom_stages, multi-detector, re-encode/smart-cut, doctor, relocate, watch mode. Kept: scan/run/status/approve/reset/clean/one. |
| **Two curation grafts** | `vsplit one <file>` single-file shortcut, plus `review.default_keep: false` config so detected scenes pre-populate as drops. |
| **Content-hash review gate** | Not mtime — vim `:wq` bumps mtime without changes. `sha256(cuts.json)` is the actual change signal. Plus explicit `vsplit approve` for "accept as-is." |
| **Stage base class with `recover()`** | Crash recovery contract: on startup, the orchestrator finds any stage in `status=running` and calls its `recover()` before normal dispatch. Default impl deletes the stage's output subdir; `split` overrides to validate per-file outputs. |
| **Filter grammar locked** | `key=value` only, keys = `status` / `stage` / `slug`, AND via repeating `--filter`, bare positional arg = slug glob. No OR/nesting/negation. |
| **Single Python process, threads** | PySceneDetect frees the GIL; threads are enough. No `multiprocessing`, no daemon, no IPC. State lives on disk. |

## Gotchas

- **No PySceneDetect/FFmpeg checks in v1.** Tasks assume they're installed and on PATH. CI must install both.
- **Test fixtures must be generated.** `tests/fixtures/*.mp4` and `*.mkv` come from running `python tests/fixtures/make_fixtures.py` once. They are `.gitignored`. Stage tests `pytest.skip` if missing.
- **Task 23 (`approve` command) does ad-hoc filter parsing** — predates Task 27's shared `parse_filter`. Deliberately left as-is; refactor at end if it bothers you.
- **Plan is large (~5200 lines, 36 tasks).** Don't try to read it all at once. Pick up the next unchecked task and execute its steps. Each task ends with a commit.
- **Windows-primary, macOS occasional.** Slug rules, path-length checks, and `subprocess.run(shell=False)` are non-negotiable. Don't use POSIX-only assumptions (symlinks, hardlinks).
- **User's CLAUDE.md is strict on scope.** Do exactly what's asked — don't refactor adjacent code "while you're there." If you spot something worth fixing outside the current task, finish the task first, then mention it.
- **The original design docs in `docs/` are not the spec.** They're inputs that got reworked. The authoritative source is `docs/superpowers/specs/2026-05-21-vsplit-design.md`.

## Exact next step

1. **Confirm with the user which execution method they want** (subagent-driven or inline). The previous session's last message offered both and is awaiting their pick.

2. **Once chosen, invoke the matching skill:**
   - Subagent-driven → `superpowers:subagent-driven-development`
   - Inline → `superpowers:executing-plans`

3. **Open the plan and start at Task 1: "Initialize the package"** — `docs/superpowers/plans/2026-05-21-vsplit-v1.md`. Task 1 creates `pyproject.toml`, the `src/vsplit/` package skeleton, `.gitignore`, and a stub `vsplit --help`. It ends with `pip install -e ".[dev]"` and a commit.

4. **TDD discipline applies.** Every task: write the failing test first, watch it fail, write minimal implementation, watch it pass, commit. Don't batch commits.

## Quick orientation commands

```bash
# Where are we?
git log --oneline
git status

# Read the spec (authoritative)
less docs/superpowers/specs/2026-05-21-vsplit-design.md

# Open the plan
less docs/superpowers/plans/2026-05-21-vsplit-v1.md

# Sanity check toolchain (these must be installed)
python --version    # >= 3.11
ffmpeg -version
ffprobe -version
```

## Pointers

- **Spec section index:** §1 goals, §2 stages, §3 schemas, §4 review gate (most subtle), §5 dispatcher state machine, §6 CLI, §7 layout/structure, §8 open questions.
- **Plan task list:** Tasks 1–13 are foundations (no stage code yet). Task 16 is the first stage (probe). Task 21–22 implement the review gate (the spec's most-thought-through area). Task 28 is the orchestrator. Tasks 33–34 are integration tests.
- **If the user pivots:** the spec is in git history (commit `39add96`). The plan is the executable expression of the spec, not a contract — if a Task gets out of step with the spec, the spec wins.
