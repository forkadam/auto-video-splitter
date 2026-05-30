# Workspace "Ignore" action — Design Spec

> Status: approved for planning (2026-05-30).
> Adds a reversible, workspace-level **Ignore** action to the vsplit dashboard so a
> user can park/abandon a workspace they don't want to process, with a distinct
> visual treatment and a filter to isolate them.

---

## 1. Motivation

The dashboard lists every adopted workspace. In a batch of 50+ videos, some are
inevitably ones the user decides not to process (junk footage, duplicates, wrong
content). Today there is no way to set such a workspace aside: it keeps showing an
active "Run: …" next-action pill and clutters the working list.

We want a one-click **Ignore** that parks a whole workspace — recoloring it,
suppressing its nagging next-action pill, and making it filterable — while staying
fully reversible (**Unignore**) and never blocking explicit actions.

## 2. Terminology

We use **Ignore / Unignore** deliberately, *not* "Skip". A per-stage status
`"skipped"` already exists in the manifest (`StageStatus`) and means "this stage was
bypassed, continue the pipeline." Reusing "skip" for a workspace-level park would
create two different "skipped" concepts in the UI and code. "Ignore" is a distinct,
reversible, workspace-level flag.

## 3. Scope

**In scope**
- A workspace-level `ignored` flag in the manifest.
- Core actions `ignore_workspaces` / `unignore_workspaces`.
- Dashboard endpoints `POST /api/workspaces/{slug}/ignore` and `/unignore`.
- Per-workspace **Ignore / Unignore** button in the expanded row.
- Distinct dimmed visual treatment + "Ignored" badge on ignored rows.
- An `ignored` value in the status filter.
- Collect-all (`collect_outputs(slugs=None)`) excludes ignored workspaces by default.

**Out of scope (YAGNI)**
- Batch Ignore/Unignore in the multi-select toolbar (explicitly declined).
- A CLI `vsplit ignore` command. The core action is reusable if added later, but no
  CLI wiring in this change.
- Ignore blocking or cancelling in-progress or explicitly-invoked runs.
- An ignore reason/note field.

## 4. Behavioral decisions (locked)

| Decision | Choice |
|---|---|
| What Ignore means | Park/abandon the whole workspace (workspace-level, reversible). |
| Representation | Workspace-level flag in the manifest (Approach A). |
| Batch toolbar button | No — per-workspace expanded-row button only. |
| Default-view visibility | Ignored workspaces stay visible, just recolored. |
| Blocking explicit actions | No — Run/Approve/Reset/Clean stay runnable on an ignored workspace without unignoring first. |
| Implicit "operate on all" | Collect-all skips ignored workspaces by default. |

## 5. Data model

`src/vsplit/schemas/manifest.py` — add two top-level fields to `Manifest`:

```python
ignored: bool = False
ignored_at: datetime | None = None
```

Both default such that existing manifests load unchanged (`extra="forbid"` is
unaffected — these are declared fields with defaults). This follows the design doc's
manifest-evolution rule (§5.2: "add a field, old workspaces still work").

`ignored_at` is recorded for inspectability (when was it parked); it is set to the
current UTC ISO timestamp on ignore and cleared to `None` on unignore.

## 6. Core actions

`src/vsplit/core/actions.py` — new helpers mirroring the existing
`approve_workspaces` shape (load manifest, mutate, save; silent skip for missing
slugs; return count actually changed):

```python
def ignore_workspaces(working_root: Path, slugs: list[str]) -> int:
    """Set ignored=True + ignored_at=now for each workspace. Returns count changed."""

def unignore_workspaces(working_root: Path, slugs: list[str]) -> int:
    """Set ignored=False + ignored_at=None for each workspace. Returns count changed."""
```

Idempotent: ignoring an already-ignored workspace is a no-op that does not count.

**Collect-all exclusion** (`src/vsplit/core/collect.py`): when `collect_outputs` is
called with `slugs=None` (collect everything), workspaces whose manifest has
`ignored=True` are excluded. The result dict gains a `skipped_ignored` count so the
dashboard can report it. When `slugs` is provided explicitly, ignored workspaces are
**not** excluded (explicit intent wins).

`run_workspaces` and `approve_workspaces` are **not** changed — they remain runnable
on ignored workspaces (explicit actions are never blocked).

## 7. Dashboard API

`src/vsplit/dashboard/api/actions.py` — two new single-workspace endpoints following
the existing `approve_one` / `clean_one` pattern exactly (resolve root,
`_require_slug`, `run_in_threadpool` the core action, return a count):

```
POST /api/workspaces/{slug}/ignore    -> {"ignored": <int>}
POST /api/workspaces/{slug}/unignore  -> {"ignored": <int>}
```

The endpoints do **not** publish SSE themselves. Like `approve`/`clean`, they mutate
`manifest.json`; the background **watcher thread** (`dashboard/watcher.py`, polls
manifest mtime every 500ms) detects the change and emits the `workspace_updated`
event that drives the list to refetch. No batch endpoints.

`src/vsplit/dashboard/api/workspaces.py` — `_summary` adds `"ignored": m.ignored` to
the returned dict so both `GET /api/workspaces` and `GET /api/workspaces/{slug}`
expose it.

## 8. Frontend

### 8.1 API client — `src/vsplit/dashboard/src/src/lib/api.ts`
- `Workspace` type gains `ignored: boolean`.
- Add `ignore: (slug) => POST /api/workspaces/{slug}/ignore` and
  `unignore: (slug) => POST /api/workspaces/{slug}/unignore`.

### 8.2 Next-action — `src/.../lib/nextAction.ts`
- Add a new variant `{ kind: "ignored"; label: "Ignored" }`.
- At the top of `nextAction`, short-circuit: `if (ws.ignored) return { kind: "ignored", label: "Ignored" }`.
  This takes precedence over everything (including `all_done`) so a parked workspace
  never shows a "Run: …" / "Review" pill.

### 8.3 Row — `src/.../views/WorkspaceRow.tsx`
- **Card treatment when `ws.ignored`:** add `opacity-60` and a slate tint
  (`bg-zinc-800/40 border-zinc-700`) to the outer card so it visually recedes.
  Selection/preview ring styles still apply on top.
- **NextPill:** render the `"ignored"` kind as a muted pill (same styling family as
  `idle`: `bg-muted text-muted-foreground`) reading "Ignored".
- **Expanded action row:** show **Unignore** (`variant="ghost"`) when `ws.ignored`,
  otherwise **Ignore** (`variant="outline"`). Wire to `api.workspaces.unignore` /
  `ignore`. Place it after Reset and before/near Clean.
- Run / Reset / Clean / Open preview remain enabled regardless of `ignored`.

### 8.4 Filter — `src/.../views/FilterBar.tsx` + `src/.../App.tsx`
- In `FilterBar`, replace the existing `"skipped"` entry in `STATUSES` with
  `"ignored"`. (The per-stage `"skipped"` value was never a settable workspace state;
  removing it from the dropdown avoids the two-meanings ambiguity.)
- In `App.tsx` `matchesFilter`: add a branch `if (f.status === "ignored") return ws.ignored`.
  For every other status value (including `"any"`), do **not** auto-exclude ignored
  workspaces — they stay visible, just recolored.
- The `stage` filter and `slug` glob are unchanged. Note `matchesFilter`'s existing
  `stage`-filter logic already treats per-stage `"skipped"` as complete; that logic is
  untouched and unrelated to the new workspace-level flag.

## 9. Build / artifacts

The dashboard ships a prebuilt `dist/` (see the `build(dashboard): rebuild dist`
commits). After the frontend changes, rebuild `dist` and commit it as a separate
`build(dashboard): rebuild dist` commit, consistent with repo convention.

## 10. Testing

- **Schema:** a manifest without `ignored` loads with `ignored=False`,
  `ignored_at=None`; round-trips after set/clear.
- **Core actions:** `ignore_workspaces` sets the flag + timestamp and returns the
  changed count; idempotent re-ignore returns 0; `unignore_workspaces` clears both;
  missing slugs are silently skipped.
- **Collect:** `collect_outputs(slugs=None)` excludes an ignored `split=done`
  workspace and increments `skipped_ignored`; an explicit `slugs=[that]` still
  collects it.
- **API:** `POST /ignore` then `GET /api/workspaces/{slug}` shows `ignored=true`;
  `/unignore` reverts; unknown slug → 404. (Refresh is driven by the watcher emitting
  `workspace_updated` on the manifest mtime change, as for approve/clean.)
- **Frontend (manual / existing harness):** ignored row is dimmed + badged, NextPill
  reads "Ignored", Unignore reverts, the `ignored` filter isolates them, and they
  remain visible (recolored) under `any`.

## 11. Risks / notes

- **Two "skipped"-like concepts remain in the data** (per-stage `"skipped"` status vs
  workspace `ignored`). Mitigated by distinct names; the filter dropdown only surfaces
  `ignored`, so users never see the ambiguity.
- **No interruption of running pipelines.** Ignoring a workspace mid-run only sets a
  flag; the in-flight run completes. Acceptable — ignore is an organizational marker.
- **Collect-all exclusion is the only implicit gate.** If a user expects ignore to
  also stop batch Run on explicitly-selected rows, it won't — by design (explicit
  selection wins). Documented here to set expectations.
