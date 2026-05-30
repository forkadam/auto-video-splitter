# Workspace "Ignore" Action Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reversible, workspace-level **Ignore / Unignore** action to the vsplit dashboard so a user can park a workspace they don't want to process — recolored, badge, suppressed next-action pill, filterable — without blocking explicit actions.

**Architecture:** A new `ignored` boolean flag (plus `ignored_at` timestamp) on the `Manifest` (source of truth). Core actions flip the flag; two dashboard endpoints expose them; the existing manifest watcher drives the live UI refresh. Collect-all excludes ignored workspaces; all explicit actions stay runnable. Frontend recolors the row, swaps the next-action pill for an "Ignored" pill, adds an Ignore/Unignore button, and adds an `ignored` status filter.

**Tech Stack:** Python 3.13 + Pydantic v2 + FastAPI (backend), React + TypeScript + Tailwind + shadcn/ui (dashboard), pytest (tests), `uv` for all Python invocations, `bun` for the frontend build.

**Spec:** `docs/superpowers/specs/2026-05-30-workspace-ignore-design.md`

**Conventions to follow:**
- Run all Python via `uv run` (e.g. `uv run pytest ...`).
- Backend follows TDD: failing test → run → implement → run → commit.
- Frontend has no JS unit-test harness; its tasks are implement → typecheck/build → manual verify → commit, ending with a `build(dashboard): rebuild dist` commit (repo convention — see `git log`).

---

## File Structure

**Backend (modify):**
- `src/vsplit/schemas/manifest.py` — add `ignored` / `ignored_at` fields to `Manifest`.
- `src/vsplit/core/actions.py` — add `ignore_workspaces` / `unignore_workspaces`.
- `src/vsplit/core/collect.py` — exclude ignored workspaces from collect-all; add `skipped_ignored` count.
- `src/vsplit/dashboard/api/actions.py` — add `POST /{slug}/ignore` and `/{slug}/unignore`.
- `src/vsplit/dashboard/api/workspaces.py` — `_summary` exposes `ignored`.

**Backend (tests):**
- `tests/unit/test_manifest_schema.py` — defaults + round-trip for the new fields.
- `tests/unit/test_core_actions_ignore.py` — new file.
- `tests/unit/test_core_collect.py` — ignored exclusion cases.
- `tests/unit/test_dashboard_api_actions.py` — endpoint tests.
- `tests/unit/test_dashboard_api_workspaces.py` — `_summary` exposes `ignored`.

**Frontend (modify):**
- `src/vsplit/dashboard/src/src/lib/api.ts` — `Workspace.ignored` + `ignore`/`unignore` methods.
- `src/vsplit/dashboard/src/src/lib/nextAction.ts` — `ignored` variant + short-circuit.
- `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx` — recolor, pill, Ignore/Unignore button.
- `src/vsplit/dashboard/src/src/views/FilterBar.tsx` — replace `skipped` with `ignored` in dropdown.
- `src/vsplit/dashboard/src/src/App.tsx` — `matchesFilter` `ignored` branch.
- `src/vsplit/dashboard/dist/**` — rebuilt artifacts (committed).

---

## Task 1: Manifest schema — `ignored` / `ignored_at` fields

**Files:**
- Modify: `src/vsplit/schemas/manifest.py`
- Test: `tests/unit/test_manifest_schema.py`

- [ ] **Step 1: Write the failing test**

Append to `tests/unit/test_manifest_schema.py`:

```python
def test_manifest_ignored_defaults_false_when_absent():
    """Existing manifests (no `ignored` key) load with ignored=False, ignored_at=None."""
    data = {
        "schema_version": 1,
        "id": "birthday",
        "created_at": _now(),
        "updated_at": _now(),
        "source": {
            "relative_path": "Birthday.mov",
            "size_bytes": 100,
            "mtime": _now(),
            "partial_hash": "sha256:abc",
        },
        "stages": {
            name: {"status": "pending", "artifacts": {}}
            for name in ["probe", "detect", "preview", "review", "plan", "split", "verify"]
        },
    }
    m = Manifest.model_validate(data)
    assert m.ignored is False
    assert m.ignored_at is None


def test_manifest_ignored_round_trips():
    data = {
        "schema_version": 1,
        "id": "birthday",
        "created_at": _now(),
        "updated_at": _now(),
        "ignored": True,
        "ignored_at": _now(),
        "source": {
            "relative_path": "Birthday.mov",
            "size_bytes": 100,
            "mtime": _now(),
            "partial_hash": "sha256:abc",
        },
        "stages": {
            name: {"status": "pending", "artifacts": {}}
            for name in ["probe", "detect", "preview", "review", "plan", "split", "verify"]
        },
    }
    m = Manifest.model_validate(data)
    assert m.ignored is True
    assert m.ignored_at is not None
    # Survives a dump/reload cycle.
    m2 = Manifest.model_validate(m.model_dump(mode="json"))
    assert m2.ignored is True
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/unit/test_manifest_schema.py::test_manifest_ignored_defaults_false_when_absent -v`
Expected: FAIL — `AttributeError: 'Manifest' object has no attribute 'ignored'`.

- [ ] **Step 3: Add the fields**

In `src/vsplit/schemas/manifest.py`, inside `class Manifest`, add the two fields immediately after the `source: SourceInfo` line (before `metadata`):

```python
    source: SourceInfo
    ignored: bool = False
    ignored_at: datetime | None = None
    metadata: MetadataBlock = Field(default_factory=MetadataBlock)
```

(`datetime` is already imported at the top of the file.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_manifest_schema.py -v`
Expected: PASS (all tests, including the two new ones).

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/schemas/manifest.py tests/unit/test_manifest_schema.py
git commit -m "feat(manifest): add workspace-level ignored flag"
```

---

## Task 2: Core actions — `ignore_workspaces` / `unignore_workspaces`

**Files:**
- Modify: `src/vsplit/core/actions.py`
- Test: `tests/unit/test_core_actions_ignore.py` (create)

- [ ] **Step 1: Write the failing test**

Create `tests/unit/test_core_actions_ignore.py`:

```python
from datetime import datetime, timezone
from pathlib import Path

from vsplit.core.actions import ignore_workspaces, unignore_workspaces
from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> Path:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)
    return ws


def test_ignore_sets_flag_and_timestamp(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    count = ignore_workspaces(tmp_path, ["alpha"])
    assert count == 1
    m = load_manifest(ws)
    assert m.ignored is True
    assert m.ignored_at is not None


def test_ignore_is_idempotent(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    ignore_workspaces(tmp_path, ["alpha"])
    # Re-ignoring an already-ignored workspace changes nothing and counts 0.
    assert ignore_workspaces(tmp_path, ["alpha"]) == 0


def test_unignore_clears_flag_and_timestamp(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    ignore_workspaces(tmp_path, ["alpha"])
    count = unignore_workspaces(tmp_path, ["alpha"])
    assert count == 1
    m = load_manifest(ws)
    assert m.ignored is False
    assert m.ignored_at is None


def test_unignore_on_not_ignored_is_noop(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    assert unignore_workspaces(tmp_path, ["alpha"]) == 0


def test_ignore_skips_missing_slugs(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    # "ghost" has no workspace dir/manifest — silently skipped.
    assert ignore_workspaces(tmp_path, ["alpha", "ghost"]) == 1
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/unit/test_core_actions_ignore.py -v`
Expected: FAIL — `ImportError: cannot import name 'ignore_workspaces'`.

- [ ] **Step 3: Implement the actions**

In `src/vsplit/core/actions.py`, add these two functions after `approve_workspaces` (the `datetime`, `timezone`, `load_manifest`, `save_manifest`, `workspace_dir` imports are already present at the top of the file):

```python
def ignore_workspaces(working_root: Path, slugs: list[str]) -> int:
    """Mark each workspace as ignored (parked). Returns the number changed.

    Idempotent: a workspace already ignored is skipped and not counted. Missing
    workspaces (no manifest) are silently skipped.
    """
    now = datetime.now(timezone.utc).isoformat()
    changed = 0
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        try:
            m = load_manifest(ws)
        except FileNotFoundError:
            continue
        if m.ignored:
            continue
        m.ignored = True
        m.ignored_at = now
        save_manifest(ws, m)
        changed += 1
    return changed


def unignore_workspaces(working_root: Path, slugs: list[str]) -> int:
    """Clear the ignored flag on each workspace. Returns the number changed.

    Idempotent: a workspace that isn't ignored is skipped and not counted.
    """
    changed = 0
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        try:
            m = load_manifest(ws)
        except FileNotFoundError:
            continue
        if not m.ignored:
            continue
        m.ignored = False
        m.ignored_at = None
        save_manifest(ws, m)
        changed += 1
    return changed
```

Note: `m.ignored_at` is assigned an ISO string; Pydantic coerces it to `datetime` on the next load, matching how `approve_workspaces` stores `approved_at`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_core_actions_ignore.py -v`
Expected: PASS (all 5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/core/actions.py tests/unit/test_core_actions_ignore.py
git commit -m "feat(core): ignore_workspaces / unignore_workspaces actions"
```

---

## Task 3: Collect-all excludes ignored workspaces

**Files:**
- Modify: `src/vsplit/core/collect.py`
- Test: `tests/unit/test_core_collect.py`

- [ ] **Step 1: Write the failing test**

Append to `tests/unit/test_core_collect.py` (the `_seed`, `ignore_workspaces`/manifest helpers are imported below; `_seed` already exists in this file). Add an import line near the top imports:

```python
from vsplit.core.actions import ignore_workspaces
```

Then append the tests:

```python
def test_collect_all_excludes_ignored(tmp_path: Path) -> None:
    """With no slugs, an ignored split=done workspace is skipped and counted."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    _seed(tmp_path, "beta", clip_names=["beta-01.mp4"])
    ignore_workspaces(tmp_path, ["beta"])

    result = collect_outputs(tmp_path)

    dest = tmp_path / "collected"
    assert (dest / "alpha-01.mp4").exists()
    assert not (dest / "beta-01.mp4").exists()
    assert result["collected"] == 1
    assert result["skipped_ignored"] == 1


def test_collect_explicit_slugs_includes_ignored(tmp_path: Path) -> None:
    """Naming an ignored workspace explicitly still collects it (explicit intent)."""
    _seed(tmp_path, "beta", clip_names=["beta-01.mp4"])
    ignore_workspaces(tmp_path, ["beta"])

    result = collect_outputs(tmp_path, slugs=["beta"])

    assert (tmp_path / "collected" / "beta-01.mp4").exists()
    assert result["collected"] == 1
    assert result["skipped_ignored"] == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/unit/test_core_collect.py::test_collect_all_excludes_ignored -v`
Expected: FAIL — `KeyError: 'skipped_ignored'` (and beta would be collected).

- [ ] **Step 3: Implement the exclusion**

In `src/vsplit/core/collect.py`, inside `collect_outputs`:

a) Record whether this is a collect-all call. Change:

```python
    idx = load_index(working_root)
    candidate_slugs = list(idx.workspaces.keys()) if slugs is None else list(slugs)
```

to:

```python
    collect_all = slugs is None
    idx = load_index(working_root)
    candidate_slugs = list(idx.workspaces.keys()) if slugs is None else list(slugs)
```

b) Add the counter alongside the other counters:

```python
    collected = 0
    from_workspaces = 0
    skipped_not_done = 0
    skipped_no_output = 0
    skipped_existing = 0
    skipped_ignored = 0
    files: list[str] = []
```

c) In the loop, after the manifest is loaded and before the `split` status check, add the exclusion:

```python
        try:
            m = load_manifest(ws)
        except FileNotFoundError:
            continue

        if collect_all and m.ignored:
            skipped_ignored += 1
            continue

        if m.stages["split"].status != "done":
            skipped_not_done += 1
            continue
```

d) Add `skipped_ignored` to the returned dict:

```python
    return {
        "collected": collected,
        "from_workspaces": from_workspaces,
        "skipped_not_done": skipped_not_done,
        "skipped_no_output": skipped_no_output,
        "skipped_existing": skipped_existing,
        "skipped_ignored": skipped_ignored,
        "files": files,
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_core_collect.py -v`
Expected: PASS (existing collect tests still pass; the two new ones pass).

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/core/collect.py tests/unit/test_core_collect.py
git commit -m "feat(collect): exclude ignored workspaces from collect-all"
```

---

## Task 4: Dashboard endpoints + `_summary` exposes `ignored`

**Files:**
- Modify: `src/vsplit/dashboard/api/actions.py`
- Modify: `src/vsplit/dashboard/api/workspaces.py`
- Test: `tests/unit/test_dashboard_api_actions.py`
- Test: `tests/unit/test_dashboard_api_workspaces.py`

- [ ] **Step 1: Write the failing endpoint test**

Append to `tests/unit/test_dashboard_api_actions.py` (the `_seed`, `TestClient`, `create_app`, `load_manifest`, `workspace_dir` are already imported in this file):

```python
def test_ignore_and_unignore_endpoints(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))

    r = client.post("/api/workspaces/alpha/ignore")
    assert r.status_code == 200
    assert r.json() == {"ignored": 1}
    assert load_manifest(workspace_dir(tmp_path, "alpha")).ignored is True

    r = client.post("/api/workspaces/alpha/unignore")
    assert r.status_code == 200
    assert r.json() == {"ignored": 1}
    assert load_manifest(workspace_dir(tmp_path, "alpha")).ignored is False


def test_ignore_unknown_slug_404(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.post("/api/workspaces/ghost/ignore")
    assert r.status_code == 404
```

- [ ] **Step 2: Write the failing `_summary` test**

`tests/unit/test_dashboard_api_workspaces.py` already defines a `_seed(tmp_path, slug)` helper and imports `TestClient` + `create_app`. Append this test (it reuses that helper):

```python
def test_list_exposes_ignored_flag(tmp_path: Path) -> None:
    from vsplit.core.actions import ignore_workspaces
    _seed(tmp_path, "alpha")
    ignore_workspaces(tmp_path, ["alpha"])
    r = TestClient(create_app(tmp_path)).get("/api/workspaces")
    assert r.status_code == 200
    ws = next(w for w in r.json()["workspaces"] if w["slug"] == "alpha")
    assert ws["ignored"] is True
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `uv run pytest tests/unit/test_dashboard_api_actions.py::test_ignore_and_unignore_endpoints tests/unit/test_dashboard_api_workspaces.py::test_list_exposes_ignored_flag -v`
Expected: FAIL — 404 / 405 on the new routes, and `KeyError: 'ignored'` on the summary.

- [ ] **Step 4: Add the endpoints**

In `src/vsplit/dashboard/api/actions.py`:

a) Extend the import from `vsplit.core.actions`:

```python
from vsplit.core.actions import (
    approve_workspaces, clean_workspaces, ignore_workspaces,
    reset_workspaces, run_workspaces, unignore_workspaces,
)
```

b) Add two endpoints after `approve_one` (mirrors that handler exactly):

```python
@router.post("/{slug}/ignore")
async def ignore_one(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    n = await run_in_threadpool(ignore_workspaces, root, [slug])
    return {"ignored": n}


@router.post("/{slug}/unignore")
async def unignore_one(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    n = await run_in_threadpool(unignore_workspaces, root, [slug])
    return {"ignored": n}
```

- [ ] **Step 5: Expose `ignored` in `_summary`**

In `src/vsplit/dashboard/api/workspaces.py`, in `_summary`, add the `ignored` key to the returned dict (e.g. right after `"slug": slug,`):

```python
    return {
        "slug": slug,
        "ignored": m.ignored,
        "source": m.source.relative_path,
        ...
```

(Leave the rest of the dict unchanged.)

- [ ] **Step 6: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_dashboard_api_actions.py tests/unit/test_dashboard_api_workspaces.py -v`
Expected: PASS.

- [ ] **Step 7: Run the full backend suite (no regressions)**

Run: `uv run pytest -q`
Expected: PASS (all tests).

- [ ] **Step 8: Commit**

```bash
git add src/vsplit/dashboard/api/actions.py src/vsplit/dashboard/api/workspaces.py tests/unit/test_dashboard_api_actions.py tests/unit/test_dashboard_api_workspaces.py
git commit -m "feat(dashboard-api): ignore/unignore endpoints + ignored in summary"
```

---

## Task 5: Frontend — type, next-action, row, filter

**Files:**
- Modify: `src/vsplit/dashboard/src/src/lib/api.ts`
- Modify: `src/vsplit/dashboard/src/src/lib/nextAction.ts`
- Modify: `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`
- Modify: `src/vsplit/dashboard/src/src/views/FilterBar.tsx`
- Modify: `src/vsplit/dashboard/src/src/App.tsx`

> No JS unit-test harness exists; verification is typecheck + build + manual.

- [ ] **Step 1: Extend the API client**

In `src/vsplit/dashboard/src/src/lib/api.ts`:

a) Add `ignored` to the `Workspace` type (after `output_count`):

```typescript
  /** Number of video clips in the workspace's output/ dir (0 until split runs). */
  output_count: number
  /** True when the user has parked this workspace via the Ignore action. */
  ignored: boolean
}
```

b) Add two methods in `api.workspaces`, next to `approve` / `clean`:

```typescript
    ignore: (slug: string) =>
      jsonRequest<{ ignored: number }>("POST", `/api/workspaces/${slug}/ignore`),
    unignore: (slug: string) =>
      jsonRequest<{ ignored: number }>("POST", `/api/workspaces/${slug}/unignore`),
```

- [ ] **Step 2: Add the `ignored` next-action**

In `src/vsplit/dashboard/src/src/lib/nextAction.ts`:

a) Add the variant to the `NextAction` union:

```typescript
export type NextAction =
  | { kind: "done"; label: "Done" }
  | { kind: "ignored"; label: "Ignored" }
  | { kind: "retry"; label: string; stage: string }
  | { kind: "running"; label: string; stage: string }
  | { kind: "review"; label: "Review & approve →" }
  | { kind: "empty"; label: "No scenes" }
  | { kind: "run"; label: string; stage: string }
  | { kind: "idle"; label: "Idle" }
```

b) Short-circuit at the very top of `nextAction` (before the `all_done` check), so a parked workspace never nags:

```typescript
export function nextAction(ws: Workspace): NextAction {
  if (ws.ignored) return { kind: "ignored", label: "Ignored" }
  if (ws.all_done) return { kind: "done", label: "Done" }
```

- [ ] **Step 3: Render the `ignored` pill**

In `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`, inside `NextPill`, add an explicit branch next to the `idle` branch (before the `onClick` button logic, so `ignored` doesn't fall through to the button render):

```typescript
  if (a.kind === "idle") {
    return <span className={cn(base, "bg-muted text-muted-foreground")}>{a.label}</span>
  }
  if (a.kind === "ignored") {
    return <span className={cn(base, "bg-zinc-700/40 text-zinc-400")}>{a.label}</span>
  }
```

- [ ] **Step 4: Recolor the ignored row**

In `WorkspaceRow.tsx`, add an ignored treatment to the outer card `className`:

```typescript
    <div className={cn(
      "rounded-lg border bg-card text-card-foreground transition-colors",
      "hover:bg-accent/40",
      ws.ignored && "opacity-60 bg-zinc-800/40 border-zinc-700",
      selected && "ring-2 ring-primary/60 border-primary/40",
      previewSlug === ws.slug && "ring-2 ring-blue-400/60 border-blue-400/40",
    )}>
```

- [ ] **Step 5: Add the Ignore / Unignore button**

In `WorkspaceRow.tsx`, in the expanded action row (the `<div className="flex gap-2 flex-wrap">` block), add the button between the Reset and Clean buttons:

```tsx
            <Button size="sm" variant="outline" onClick={() => setResetOpen(true)}>
              Reset…
            </Button>
            {ws.ignored ? (
              <Button size="sm" variant="ghost"
                      onClick={() => { void api.workspaces.unignore(ws.slug) }}>
                Unignore
              </Button>
            ) : (
              <Button size="sm" variant="outline"
                      onClick={() => { void api.workspaces.ignore(ws.slug) }}>
                Ignore
              </Button>
            )}
            <Button size="sm" variant="destructive" onClick={() => {
```

(The watcher emits `workspace_updated` on the manifest write, so the list refreshes within ~500ms — no manual refetch needed, matching the existing Clean button.)

- [ ] **Step 6: Add `ignored` to the status filter dropdown**

In `src/vsplit/dashboard/src/src/views/FilterBar.tsx`, replace `"skipped"` with `"ignored"` in `STATUSES`:

```typescript
const STATUSES = [
  "any", "pending", "running", "done", "failed", "awaiting_user", "ignored", "no_scenes",
]
```

- [ ] **Step 7: Add the `matchesFilter` branch**

In `src/vsplit/dashboard/src/src/App.tsx`, inside `matchesFilter`, add an `ignored` branch within the `if (f.status !== "any")` block — place it before the `no_scenes` branch:

```typescript
  if (f.status !== "any") {
    if (f.status === "ignored") {
      if (!ws.ignored) return false
    } else if (f.status === "done") {
      if (!ws.all_done) return false
    } else if (f.status === "no_scenes") {
```

(For every other status value — including `"any"` — ignored workspaces are NOT excluded: they stay visible, just recolored, per the spec.)

- [ ] **Step 8: Typecheck and build the frontend**

Run (from `src/vsplit/dashboard/src`):

```bash
cd src/vsplit/dashboard/src && bun install && bun run build
```

Expected: `tsc -b` compiles with no TypeScript errors, then `vite build` writes assets. Per `vite.config.ts`, `outDir` is `../dist` with `emptyOutDir: true`, so the build rewrites the served `src/vsplit/dashboard/dist` in place — no copying needed.

- [ ] **Step 9: Manual verification**

Run the dashboard against a working folder with at least two adopted workspaces:

```bash
uv run vsplit -w <some-working-folder> dashboard
```

Confirm, in the browser:
1. Expand a workspace → an **Ignore** button appears next to Reset.
2. Click **Ignore** → within ~1s the row dims, the next-action pill reads **Ignored**, and the expanded button now reads **Unignore**.
3. Set the status filter to **ignored** → only the ignored workspace(s) show.
4. Set the filter back to **any** → the ignored workspace is still listed (recolored).
5. Click **Unignore** → row returns to normal, pill returns to its real next action.
6. The individual **Run** button still works on an ignored workspace (not blocked).

- [ ] **Step 10: Commit the source changes**

```bash
git add src/vsplit/dashboard/src/src/lib/api.ts src/vsplit/dashboard/src/src/lib/nextAction.ts src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx src/vsplit/dashboard/src/src/views/FilterBar.tsx src/vsplit/dashboard/src/src/App.tsx
git commit -m "feat(dashboard-fe): workspace Ignore/Unignore action + filter"
```

---

## Task 6: Rebuild and commit dist

**Files:**
- Modify: `src/vsplit/dashboard/dist/**` (built artifacts)

- [ ] **Step 1: Confirm dist is rebuilt**

The Task 5 Step 8 build wrote fresh artifacts directly into the served `src/vsplit/dashboard/dist` (Vite `outDir`). Verify the dist-present test still passes:

Run: `uv run pytest tests/unit/test_dashboard_dist_present.py -v`
Expected: PASS.

- [ ] **Step 2: Commit the rebuilt dist**

```bash
git add src/vsplit/dashboard/dist
git commit -m "build(dashboard): rebuild dist"
```

---

## Final verification

- [ ] **Run the full backend suite:**

Run: `uv run pytest -q`
Expected: PASS — all tests, no regressions.

- [ ] **Confirm the spec is fully covered** (data model, core actions, collect exclusion, endpoints, summary field, frontend row/pill/button/filter, dist rebuild). See the spec's §10 testing checklist.
