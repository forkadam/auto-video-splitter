# vsplit cuts editor + Next-action pill — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the v0.3 headline feature — an in-dashboard editor for `decisions/cuts.json` plus a per-workspace "Next action" pill — implementing the design at `docs/superpowers/specs/2026-05-24-vsplit-cuts-editor-design.md`.

**Architecture:** Two new FastAPI endpoints (`GET`/`PUT /api/workspaces/{slug}/cuts`) reusing the existing `CutsJson` Pydantic validator, `_validate_ids` cross-check against `detect/scenes.json`, and `atomic_write_text` writer. The hash returned by the read path is a freshly-computed `content_hash(decisions/cuts.json)` — same value used by the `review` stage for re-entry, so the CLI and dashboard flows stay on one backend path. A pure TS module `nextAction.ts` derives a pill spec from the existing `Workspace` summary fields, rendered on the collapsed row. A new `CutsDrawer.tsx` (shadcn Drawer, mirroring `LogDrawer`/`ConfigDrawer`) handles editing.

**Tech Stack:** Python 3.13 · FastAPI · Pydantic v2 · pytest + FastAPI `TestClient` · React 19 + Vite · Tailwind v4 + shadcn/ui · Bun (build only — end users use the committed `dist/`).

**Conventions in this repo (read once before starting):**
- Always use `uv` for Python invocations: `uv run pytest …`, `uv pip install …`. Never raw `pip`/`python`.
- Frontend dev workflow lives in `src/vsplit/dashboard/src/`. Bun commands: `bun install`, `bun run dev`, `bun run build`, `bun run gen-types`.
- `dist/` IS committed (under `src/vsplit/dashboard/dist/`). End users don't have Bun.
- Tests live under `tests/unit/` (use `tmp_path`, `TestClient`) and `tests/integration/`.
- `_require_slug` and `_summary` are existing helpers in `dashboard/api/workspaces.py` — reuse them.
- Pre-redesign drafts `docs/vsplit-design-doc.md` and `docs/vsplit-implementation-spec.md` are NOT authoritative — ignore them. The HANDOFF.md explicitly warns about this trap.

---

## Task 1: Backend — `GET /api/workspaces/{slug}/cuts`

**Files:**
- Modify: `src/vsplit/dashboard/api/workspaces.py` (add endpoint + helper)
- Create: `tests/unit/test_dashboard_api_cuts.py`

- [ ] **Step 1: Write the failing tests**

Create `tests/unit/test_dashboard_api_cuts.py`:

```python
import json
from datetime import datetime, timezone
from pathlib import Path

from fastapi.testclient import TestClient

from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import WorkspaceEntry
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


def _seed_cuts(ws: Path, *, scene_ids: list[int]) -> Path:
    """Write detect/scenes.json + decisions/cuts.json with matching IDs."""
    (ws / "detect").mkdir(parents=True, exist_ok=True)
    (ws / "detect" / "scenes.json").write_text(json.dumps({
        "schema_version": 1,
        "scenes": [{"id": i, "start_time": float(i), "end_time": float(i) + 1.0}
                   for i in scene_ids],
    }))
    (ws / "decisions").mkdir(parents=True, exist_ok=True)
    cuts = {
        "schema_version": 1,
        "scenes": [
            {"id": i, "start": f"00:00:0{i}.000", "end": f"00:00:0{i+1}.000",
             "keep": True, "label": None, "merge_with_next": False}
            for i in scene_ids
        ],
    }
    (ws / "decisions" / "cuts.json").write_text(json.dumps(cuts, indent=2) + "\n")
    return ws / "decisions" / "cuts.json"


def test_get_cuts_happy_path(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/cuts")
    assert r.status_code == 200
    body = r.json()
    assert body["cuts"]["schema_version"] == 1
    assert len(body["cuts"]["scenes"]) == 2
    assert body["cuts"]["scenes"][0]["id"] == 1
    assert isinstance(body["hash"], str) and body["hash"].startswith("sha256:")
    assert isinstance(body["scene_thumbs"], dict)


def test_get_cuts_404_unknown_slug(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/nope/cuts")
    assert r.status_code == 404


def test_get_cuts_404_when_review_not_run(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")  # no cuts.json
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/cuts")
    assert r.status_code == 404


def test_get_cuts_scene_thumbs_first_match(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])
    img_dir = ws / "preview" / "images"
    img_dir.mkdir(parents=True)
    # Multiple thumbs per scene; expect first sorted match per id.
    (img_dir / "scene-001-00.jpg").write_bytes(b"a")
    (img_dir / "scene-001-01.jpg").write_bytes(b"b")
    (img_dir / "scene-002-00.jpg").write_bytes(b"c")
    # Decoy: scene-003 has no cuts entry.
    (img_dir / "scene-003-00.jpg").write_bytes(b"d")
    client = TestClient(create_app(tmp_path))
    body = client.get("/api/workspaces/alpha/cuts").json()
    assert body["scene_thumbs"] == {"1": "scene-001-00.jpg", "2": "scene-002-00.jpg"}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -v`
Expected: 4 FAILED (route not defined → 404 on every request, no `cuts` key in body).

- [ ] **Step 3: Implement the endpoint**

Edit `src/vsplit/dashboard/api/workspaces.py`. Add these imports near the top with the others:

```python
import json
from vsplit.core.hash import content_hash
from vsplit.schemas.cuts import CutsJson
```

Add this helper near `_list_thumbs`:

```python
def _scene_thumbs(root: Path, slug: str, scene_ids: list[int]) -> dict[str, str]:
    """For each scene id, the first sorted thumbnail filename matching scene-{id:03d}-*.jpg."""
    img_dir = workspace_dir(root, slug) / "preview" / "images"
    if not img_dir.is_dir():
        return {}
    all_names = sorted(p.name for p in img_dir.iterdir()
                       if p.is_file() and p.suffix.lower() == ".jpg")
    out: dict[str, str] = {}
    for sid in scene_ids:
        prefix = f"scene-{sid:03d}-"
        for name in all_names:
            if name.startswith(prefix):
                out[str(sid)] = name
                break
    return out
```

Add the route below the existing `open_preview` route:

```python
@router.get("/{slug}/cuts")
def get_cuts(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    cuts_path = workspace_dir(root, slug) / "decisions" / "cuts.json"
    if not cuts_path.is_file():
        raise HTTPException(status_code=404, detail="cuts.json not yet written")
    raw = cuts_path.read_text()
    parsed = CutsJson.model_validate_json(raw)
    scene_ids = [s.id for s in parsed.scenes]
    return {
        "cuts": json.loads(raw),
        "hash": content_hash(cuts_path),
        "scene_thumbs": _scene_thumbs(root, slug, scene_ids),
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -v`
Expected: 4 PASSED.

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/dashboard/api/workspaces.py tests/unit/test_dashboard_api_cuts.py
git commit -m "feat(dashboard-api): GET /workspaces/{slug}/cuts"
```

---

## Task 2: Backend — `PUT /api/workspaces/{slug}/cuts`

**Files:**
- Modify: `src/vsplit/dashboard/api/workspaces.py` (add endpoint)
- Modify: `tests/unit/test_dashboard_api_cuts.py` (add 5 tests)

- [ ] **Step 1: Write the failing tests**

Append to `tests/unit/test_dashboard_api_cuts.py`:

```python
def _valid_cuts_body(scene_ids: list[int]) -> dict:
    return {
        "schema_version": 1,
        "scenes": [
            {"id": i, "start": f"00:00:0{i}.000", "end": f"00:00:0{i+1}.000",
             "keep": True, "label": None, "merge_with_next": False}
            for i in scene_ids
        ],
    }


def test_put_cuts_happy_path(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    cuts_path = _seed_cuts(ws, scene_ids=[1, 2])
    client = TestClient(create_app(tmp_path))
    initial = client.get("/api/workspaces/alpha/cuts").json()
    new_cuts = _valid_cuts_body([1, 2])
    new_cuts["scenes"][0]["keep"] = False
    new_cuts["scenes"][1]["label"] = "ending"
    r = client.put("/api/workspaces/alpha/cuts",
                   json={"cuts": new_cuts, "expected_hash": initial["hash"]})
    assert r.status_code == 200
    body = r.json()
    assert body["hash"].startswith("sha256:") and body["hash"] != initial["hash"]
    on_disk = json.loads(cuts_path.read_text())
    assert on_disk["scenes"][0]["keep"] is False
    assert on_disk["scenes"][1]["label"] == "ending"


def test_put_cuts_409_on_stale_hash(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1])
    client = TestClient(create_app(tmp_path))
    r = client.put("/api/workspaces/alpha/cuts",
                   json={"cuts": _valid_cuts_body([1]),
                         "expected_hash": "sha256:0000000000000000"})
    assert r.status_code == 409
    body = r.json()
    assert body["detail"] == "cuts.json changed on disk"
    assert body["current_hash"].startswith("sha256:")


def test_put_cuts_422_schema_error(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1])
    client = TestClient(create_app(tmp_path))
    # Bad timestamp format
    bad = _valid_cuts_body([1])
    bad["scenes"][0]["start"] = "not-a-timestamp"
    r = client.put("/api/workspaces/alpha/cuts",
                   json={"cuts": bad, "expected_hash": None})
    assert r.status_code == 422


def test_put_cuts_422_on_id_drift(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])  # detect has IDs 1,2
    client = TestClient(create_app(tmp_path))
    # cuts body uses IDs 1,3 — drift
    bad = _valid_cuts_body([1, 3])
    r = client.put("/api/workspaces/alpha/cuts",
                   json={"cuts": bad, "expected_hash": None})
    assert r.status_code == 422
    assert "scene" in r.json()["detail"].lower()


def test_put_cuts_404_unknown_slug(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.put("/api/workspaces/nope/cuts",
                   json={"cuts": _valid_cuts_body([1]), "expected_hash": None})
    assert r.status_code == 404


def test_put_cuts_writes_atomically_with_trailing_newline(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    cuts_path = _seed_cuts(ws, scene_ids=[1])
    client = TestClient(create_app(tmp_path))
    initial = client.get("/api/workspaces/alpha/cuts").json()
    new_cuts = _valid_cuts_body([1])
    new_cuts["scenes"][0]["label"] = "kept"
    client.put("/api/workspaces/alpha/cuts",
               json={"cuts": new_cuts, "expected_hash": initial["hash"]})
    text = cuts_path.read_text()
    assert text.endswith("\n")
    assert "\r\n" not in text  # LF only (atomic_write_text guarantee)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -v`
Expected: 6 new tests FAIL with 405 Method Not Allowed (route not defined).

- [ ] **Step 3: Implement the endpoint**

Add these imports to `src/vsplit/dashboard/api/workspaces.py` (top, near the existing ones):

```python
from pydantic import BaseModel, ValidationError
from vsplit.core.atomic import atomic_write_text
from vsplit.stages.review import _validate_ids
```

Add a request body model near the top of the module (after imports, before the router):

```python
class _CutsPutBody(BaseModel):
    cuts: dict
    expected_hash: str | None = None
```

Add the route below `get_cuts`:

```python
@router.put("/{slug}/cuts")
def put_cuts(slug: str, body: _CutsPutBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    ws = workspace_dir(root, slug)
    cuts_path = ws / "decisions" / "cuts.json"
    scenes_path = ws / "detect" / "scenes.json"
    if not scenes_path.is_file():
        raise HTTPException(status_code=404, detail="detect/scenes.json missing")

    try:
        parsed = CutsJson.model_validate(body.cuts)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=str(e)) from e

    detected_ids = {s["id"] for s in json.loads(scenes_path.read_text())["scenes"]}
    try:
        _validate_ids(parsed, detected_ids)
    except ValueError as e:
        raise HTTPException(status_code=422, detail=str(e)) from e

    if body.expected_hash is not None and cuts_path.is_file():
        current = content_hash(cuts_path)
        if current != body.expected_hash:
            raise HTTPException(
                status_code=409,
                detail={"detail": "cuts.json changed on disk", "current_hash": current},
            )

    atomic_write_text(
        cuts_path,
        json.dumps(parsed.model_dump(), indent=2) + "\n",
    )
    return {"hash": content_hash(cuts_path)}
```

**Note on the 409 body shape:** FastAPI's `HTTPException(detail=...)` wraps `detail` in `{"detail": ...}`. The test asserts `body["detail"] == "cuts.json changed on disk"` and `body["current_hash"].startswith("sha256:")` — that means we need to return the structured response directly, NOT via `HTTPException`. Replace the 409 branch with:

```python
    if body.expected_hash is not None and cuts_path.is_file():
        current = content_hash(cuts_path)
        if current != body.expected_hash:
            from fastapi.responses import JSONResponse
            return JSONResponse(
                status_code=409,
                content={"detail": "cuts.json changed on disk", "current_hash": current},
            )
```

(FastAPI accepts a `Response`/`JSONResponse` return value even from a typed endpoint.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -v`
Expected: 10 PASSED (4 from Task 1 + 6 new).

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/dashboard/api/workspaces.py tests/unit/test_dashboard_api_cuts.py
git commit -m "feat(dashboard-api): PUT /workspaces/{slug}/cuts"
```

---

## Task 3: Frontend lib — `nextAction.ts` + cuts API helpers

**Files:**
- Create: `src/vsplit/dashboard/src/src/lib/nextAction.ts`
- Modify: `src/vsplit/dashboard/src/src/lib/api.ts`

- [ ] **Step 1: Implement the pure state-machine module**

Create `src/vsplit/dashboard/src/src/lib/nextAction.ts`:

```typescript
import type { Workspace } from "./api"

const ORDER = ["probe", "detect", "preview", "review", "plan", "split", "verify"] as const

export type NextAction =
  | { kind: "done"; label: "Done" }
  | { kind: "retry"; label: string; stage: string }
  | { kind: "running"; label: string; stage: string }
  | { kind: "review"; label: "Review & approve →" }
  | { kind: "run"; label: string; stage: string }
  | { kind: "idle"; label: "Idle" }

export function nextAction(ws: Workspace): NextAction {
  if (ws.all_done) return { kind: "done", label: "Done" }

  for (const s of ORDER) {
    if (ws.stages[s] === "failed") {
      return { kind: "retry", label: `Retry: ${s} →`, stage: s }
    }
  }

  for (const s of ORDER) {
    if (ws.stages[s] === "running") {
      return { kind: "running", label: `Running: ${s}…`, stage: s }
    }
  }

  if (ws.stages.review === "awaiting_user") {
    return { kind: "review", label: "Review & approve →" }
  }

  for (const s of ORDER) {
    if (ws.stages[s] !== "done" && ws.stages[s] !== "skipped") {
      return { kind: "run", label: `Run: ${s} →`, stage: s }
    }
  }

  return { kind: "idle", label: "Idle" }
}
```

- [ ] **Step 2: Add cuts API helpers**

Edit `src/vsplit/dashboard/src/src/lib/api.ts`. Add a type and three methods to the `workspaces` object (alphabetical-ish placement, just after `clean`):

```typescript
export type CutsScene = {
  id: number
  start: string
  end: string
  keep: boolean
  label: string | null
  merge_with_next: boolean
}

export type CutsBody = {
  schema_version: number
  scenes: CutsScene[]
}

export type CutsResponse = {
  cuts: CutsBody
  hash: string
  scene_thumbs: Record<string, string>
}
```

Add the methods inside `api.workspaces`:

```typescript
    cuts: {
      get: (slug: string) =>
        jsonRequest<CutsResponse>("GET", `/api/workspaces/${slug}/cuts`),
      put: (slug: string, body: { cuts: CutsBody; expected_hash: string | null }) =>
        jsonRequest<{ hash: string }>("PUT", `/api/workspaces/${slug}/cuts`, body as unknown as Record<string, unknown>),
    },
```

- [ ] **Step 3: Verify TypeScript compiles**

```bash
cd src/vsplit/dashboard/src
bun run build
```

Expected: build completes with no TS errors. (Build emits to `../dist/`; don't worry about the diff yet — we'll commit dist in Task 7.)

- [ ] **Step 4: Commit (source only, dist later)**

```bash
git add src/vsplit/dashboard/src/src/lib/nextAction.ts src/vsplit/dashboard/src/src/lib/api.ts
git commit -m "feat(dashboard-fe): nextAction state machine + cuts API helpers"
```

---

## Task 4: Frontend — `CutsDrawer` component (load + render + Save)

**Files:**
- Create: `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`

- [ ] **Step 1: Implement the drawer**

Create `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`:

```tsx
import { useEffect, useMemo, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle, DrawerFooter,
} from "@/components/ui/drawer"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { api, type CutsBody } from "@/lib/api"

type Props = {
  slug: string | null
  onClose: () => void
}

type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
}

export function CutsDrawer({ slug, onClose }: Props) {
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [draft, setDraft] = useState<CutsBody | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (!slug) {
      setLoaded(null)
      setDraft(null)
      setError(null)
      return
    }
    api.workspaces.cuts.get(slug)
      .then((r) => {
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs })
        setDraft(structuredClone(r.cuts))
        setError(null)
      })
      .catch((e) => setError(String(e.message ?? e)))
  }, [slug])

  const isDirty = useMemo(() => {
    if (!loaded || !draft) return false
    return JSON.stringify(loaded.cuts) !== JSON.stringify(draft)
  }, [loaded, draft])

  const update = (idx: number, patch: Partial<CutsBody["scenes"][number]>) => {
    setDraft((d) => {
      if (!d) return d
      const next = structuredClone(d)
      next.scenes[idx] = { ...next.scenes[idx], ...patch }
      return next
    })
  }

  const cancel = () => {
    if (isDirty && !window.confirm("Discard unsaved changes?")) return
    onClose()
  }

  const save = async (): Promise<boolean> => {
    if (!slug || !loaded || !draft) return false
    setBusy(true)
    setError(null)
    try {
      const r = await api.workspaces.cuts.put(slug, {
        cuts: draft, expected_hash: loaded.hash,
      })
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs })
      return true
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      return false
    } finally {
      setBusy(false)
    }
  }

  return (
    <Drawer open={!!slug} onOpenChange={(o) => !o && cancel()}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>{slug} · review cuts.json</DrawerTitle>
        </DrawerHeader>

        {error && (
          <div className="mx-4 mb-2 px-3 py-2 text-sm rounded-md bg-red-500/10 border border-red-500/30 text-red-400">
            {error}
          </div>
        )}

        <div className="px-4 pb-4 overflow-auto max-h-[60vh] space-y-2">
          {!loaded && !error && <div className="text-muted-foreground text-sm">Loading…</div>}
          {loaded && draft && draft.scenes.map((s, i) => {
            const thumb = loaded.thumbs[String(s.id)]
            const isLast = i === draft.scenes.length - 1
            return (
              <div key={s.id} className="flex gap-3 p-2 rounded-md border border-border bg-card">
                {thumb ? (
                  <img
                    src={api.workspaces.thumbUrl(slug!, thumb)}
                    alt=""
                    className="w-32 h-18 rounded object-cover bg-muted ring-1 ring-border flex-shrink-0"
                    loading="lazy"
                  />
                ) : (
                  <div className="w-32 h-18 rounded bg-muted ring-1 ring-border flex-shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <div className="font-mono text-xs text-muted-foreground">
                    #{s.id.toString().padStart(3, "0")}  ·  {s.start} – {s.end}
                  </div>
                  <div className="mt-1.5 flex items-center gap-3 flex-wrap">
                    <label className="flex items-center gap-1.5 text-sm">
                      <Checkbox checked={s.keep}
                                onCheckedChange={(c) => update(i, { keep: !!c })} />
                      keep
                    </label>
                    <label className="flex items-center gap-1.5 text-sm">
                      <Checkbox
                        checked={s.merge_with_next}
                        disabled={isLast}
                        onCheckedChange={(c) => update(i, { merge_with_next: !!c })}
                      />
                      merge with next
                    </label>
                    <Input
                      placeholder="label (optional)"
                      value={s.label ?? ""}
                      onChange={(e) => update(i, { label: e.target.value || null })}
                      className="h-7 w-48 text-sm"
                    />
                  </div>
                </div>
              </div>
            )
          })}
        </div>

        <DrawerFooter className="flex-row justify-end gap-2">
          <Button variant="ghost" onClick={cancel} disabled={busy}>Cancel</Button>
          <Button variant="secondary" onClick={save} disabled={busy || !isDirty}>
            Save
          </Button>
          {/* Save & continue lives here too — added in Task 5 */}
        </DrawerFooter>
      </DrawerContent>
    </Drawer>
  )
}
```

- [ ] **Step 2: Build to verify TypeScript compiles**

```bash
cd src/vsplit/dashboard/src
bun run build
```

Expected: build completes with no errors. The new file is unreferenced in `App.tsx` so there's no runtime wiring yet — that's fine.

- [ ] **Step 3: Commit**

```bash
git add src/vsplit/dashboard/src/src/views/CutsDrawer.tsx
git commit -m "feat(dashboard-fe): CutsDrawer with Save (Save & continue TBD)"
```

---

## Task 5: Frontend — `CutsDrawer` Save & continue chain

**Files:**
- Modify: `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`

- [ ] **Step 1: Add the Save & continue button + handler**

In `CutsDrawer.tsx`, replace the `DrawerFooter` block with:

```tsx
        <DrawerFooter className="flex-row justify-end gap-2">
          <Button variant="ghost" onClick={cancel} disabled={busy}>Cancel</Button>
          <Button variant="secondary" onClick={save} disabled={busy || !isDirty}>
            Save
          </Button>
          <Button onClick={saveAndContinue} disabled={busy}>
            Save & continue
          </Button>
        </DrawerFooter>
```

Add the `saveAndContinue` handler just below the existing `save` handler:

```tsx
  const saveAndContinue = async () => {
    if (!slug) return
    if (isDirty) {
      const ok = await save()
      if (!ok) return  // error already surfaced by save()
    }
    setBusy(true)
    setError(null)
    try {
      await api.workspaces.approve(slug)
      await api.workspaces.run(slug, {})
      onClose()
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
    } finally {
      setBusy(false)
    }
  }
```

- [ ] **Step 2: Build to verify**

```bash
cd src/vsplit/dashboard/src
bun run build
```

Expected: build completes with no TS errors.

- [ ] **Step 3: Commit**

```bash
git add src/vsplit/dashboard/src/src/views/CutsDrawer.tsx
git commit -m "feat(dashboard-fe): CutsDrawer Save & continue (PUT → approve → run)"
```

---

## Task 6: Frontend — `WorkspaceRow` Next-action pill + remove standalone Approve + wire drawer

**Files:**
- Modify: `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`
- Modify: `src/vsplit/dashboard/src/src/App.tsx`

- [ ] **Step 1: Update `App.tsx` to manage drawer state and pass `onOpenCuts`**

Replace the body of `App.tsx` with this (only the additions/changes; preserve all existing imports + structure):

```tsx
import { useEffect, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { useSSE } from "@/lib/sse"
import { Settings } from "lucide-react"
import { Button } from "@/components/ui/button"
import { WorkspaceRow } from "./views/WorkspaceRow"
import { FilterBar, type FilterState } from "./views/FilterBar"
import { BatchActionBar } from "./views/BatchActionBar"
import { LogDrawer } from "./views/LogDrawer"
import { ConfigDrawer } from "./views/ConfigDrawer"
import { CutsDrawer } from "./views/CutsDrawer"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"]

function matchesFilter(ws: Workspace, f: FilterState): boolean {
  if (f.status !== "any") {
    if (!Object.values(ws.stages).includes(f.status)) return false
  }
  if (f.stage !== "any") {
    const next = STAGES.find((s) => ws.stages[s] !== "done")
    if (next !== f.stage) return false
  }
  if (f.slug) {
    const pattern = new RegExp("^" + f.slug.replace(/\*/g, ".*").replace(/\?/g, ".") + "$")
    if (!pattern.test(ws.slug)) return false
  }
  return true
}

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])
  const [expanded, setExpanded] = useState<string | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [filter, setFilter] = useState<FilterState>({ status: "any", stage: "any", slug: "" })
  const [logOpen, setLogOpen] = useState<{ slug: string; stage: string } | null>(null)
  const [configOpen, setConfigOpen] = useState(false)
  const [cutsSlug, setCutsSlug] = useState<string | null>(null)

  const refetch = () => {
    api.workspaces.list().then((d) => setWorkspaces(d.workspaces))
  }
  useEffect(refetch, [])

  useSSE((ev) => {
    if (ev.type === "workspace_updated" || ev.type === "workspaces_added"
      || ev.type === "workspaces_removed" || ev.type === "run_finished") {
      refetch()
    }
  })

  const toggleExpand = (slug: string) => setExpanded((e) => e === slug ? null : slug)
  const toggleSelect = (slug: string, on: boolean) => setSelected((s) => {
    const next = new Set(s)
    if (on) next.add(slug); else next.delete(slug)
    return next
  })

  const visible = workspaces.filter((w) => matchesFilter(w, filter))

  return (
    <div className="min-h-full">
      <BatchActionBar
        selected={selected}
        onCleared={() => { setSelected(new Set()); refetch() }}
      />
      <header className="sticky top-0 z-20 backdrop-blur-md bg-background/80 border-b border-border px-6 py-3.5 flex items-center gap-4">
        <div className="size-7 rounded-md bg-gradient-to-br from-primary to-primary/40 shadow-lg shadow-primary/20" />
        <h1 className="text-base font-semibold tracking-tight">vsplit dashboard</h1>
        <div className="text-sm text-muted-foreground">
          {visible.length} of {workspaces.length} workspace{workspaces.length === 1 ? "" : "s"}
        </div>
        <Button variant="ghost" size="icon" className="ml-auto" onClick={() => setConfigOpen(true)}>
          <Settings className="h-4 w-4" />
        </Button>
      </header>
      <main className="max-w-6xl mx-auto p-6">
        <FilterBar value={filter} onChange={setFilter} />
        {workspaces.length === 0 ? (
          <div className="text-muted-foreground text-sm py-12 text-center">
            No workspaces yet. Run <code className="text-foreground bg-muted px-1.5 py-0.5 rounded">vsplit scan</code> to adopt files.
          </div>
        ) : visible.length === 0 ? (
          <div className="text-muted-foreground text-sm py-12 text-center">No workspaces match the current filter.</div>
        ) : (
          <div className="space-y-2">
            {visible.map((ws) => (
              <WorkspaceRow
                key={ws.slug}
                ws={ws}
                selected={selected.has(ws.slug)}
                onSelectChange={toggleSelect}
                expanded={expanded === ws.slug}
                onToggle={toggleExpand}
                onOpenLog={(slug, stage) => setLogOpen({ slug, stage })}
                onOpenCuts={(slug) => setCutsSlug(slug)}
              />
            ))}
          </div>
        )}
      </main>
      <LogDrawer
        slug={logOpen?.slug ?? null}
        stage={logOpen?.stage ?? null}
        onClose={() => setLogOpen(null)}
      />
      <CutsDrawer slug={cutsSlug} onClose={() => { setCutsSlug(null); refetch() }} />
      <ConfigDrawer open={configOpen} onOpenChange={setConfigOpen} />
    </div>
  )
}
```

- [ ] **Step 2: Update `WorkspaceRow.tsx` — add `onOpenCuts` prop, render the pill, remove standalone Approve button**

Replace the whole `WorkspaceRow.tsx` with:

```tsx
import { useEffect, useState } from "react"
import { ChevronRight, ChevronDown, Loader2 } from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  HoverCard, HoverCardContent, HoverCardTrigger,
} from "@/components/ui/hover-card"
import { api, type Workspace } from "@/lib/api"
import { nextAction } from "@/lib/nextAction"

const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"] as const

const STATUS_COLOR: Record<string, string> = {
  done: "bg-emerald-500",
  pending: "bg-zinc-700",
  running: "bg-blue-500 animate-pulse",
  failed: "bg-red-500",
  awaiting_user: "bg-amber-500",
  skipped: "bg-zinc-600",
}

type Props = {
  ws: Workspace
  selected: boolean
  onSelectChange: (slug: string, selected: boolean) => void
  expanded: boolean
  onToggle: (slug: string) => void
  onOpenLog: (slug: string, stage: string) => void
  onOpenCuts: (slug: string) => void
}

type Detail = {
  workspace_path?: string
  thumb_names?: string[]
  stages?: Record<string, string>
  manifest?: {
    stages?: Record<string, { artifacts?: Record<string, unknown> }>
  }
}

function NextPill({ ws, onOpenCuts }: { ws: Workspace; onOpenCuts: (s: string) => void }) {
  const a = nextAction(ws)
  const base = "px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5"
  if (a.kind === "done") {
    return <span className={cn(base, "bg-emerald-500/15 text-emerald-300")}>{a.label}</span>
  }
  if (a.kind === "running") {
    return (
      <span className={cn(base, "bg-blue-500/15 text-blue-300")}>
        <Loader2 className="h-3 w-3 animate-spin" />
        {a.label}
      </span>
    )
  }
  if (a.kind === "idle") {
    return <span className={cn(base, "bg-muted text-muted-foreground")}>{a.label}</span>
  }
  const onClick = () => {
    if (a.kind === "retry") return void api.workspaces.run(ws.slug, { only: a.stage })
    if (a.kind === "review") return onOpenCuts(ws.slug)
    if (a.kind === "run") return void api.workspaces.run(ws.slug, {})
  }
  const tone =
    a.kind === "retry" ? "bg-red-500/15 text-red-300 hover:bg-red-500/25" :
    a.kind === "review" ? "bg-amber-500/15 text-amber-300 hover:bg-amber-500/25" :
    "bg-primary/15 text-primary hover:bg-primary/25"
  return (
    <button type="button" onClick={onClick}
            className={cn(base, "cursor-pointer transition-colors", tone)}>
      {a.label}
    </button>
  )
}

export function WorkspaceRow({
  ws, selected, onSelectChange, expanded, onToggle, onOpenLog, onOpenCuts,
}: Props) {
  const [detail, setDetail] = useState<Detail | null>(null)

  useEffect(() => {
    if (!expanded) return
    api.workspaces.detail(ws.slug).then((d) => setDetail(d as unknown as Detail))
  }, [expanded, ws.slug])

  const thumbNames = detail?.thumb_names ?? []
  const previewReady = ws.stages.preview === "done"

  return (
    <div className={cn(
      "rounded-lg border bg-card text-card-foreground transition-colors",
      "hover:bg-accent/40",
      selected && "ring-2 ring-primary/60 border-primary/40",
    )}>
      <div className="w-full px-4 py-2.5 flex items-center gap-3">
        <button onClick={() => onToggle(ws.slug)} className="flex items-center gap-3 min-w-0 flex-1 text-left">
          {expanded ? <ChevronDown className="h-4 w-4 text-muted-foreground" /> : <ChevronRight className="h-4 w-4 text-muted-foreground" />}
          {ws.first_thumb ? (
            <img
              src={api.workspaces.thumbUrl(ws.slug, ws.first_thumb)}
              alt=""
              className="w-14 h-8 rounded-md object-cover bg-muted ring-1 ring-border"
              loading="lazy"
            />
          ) : (
            <div className="w-14 h-8 rounded-md bg-muted ring-1 ring-border" />
          )}
          <div className="font-medium text-sm min-w-0 truncate">{ws.slug}</div>
        </button>
        <NextPill ws={ws} onOpenCuts={onOpenCuts} />
        <Checkbox
          checked={selected}
          onCheckedChange={(c) => onSelectChange(ws.slug, !!c)}
        />
        <div className="flex gap-0.5">
          {STAGES.map((s) => {
            const status = ws.stages[s] ?? "pending"
            const isFailed = status === "failed"
            return (
              <button
                key={s}
                type="button"
                title={`${s}: ${status}${isFailed ? " (click to view log)" : ""}`}
                onClick={isFailed ? () => onOpenLog(ws.slug, s) : undefined}
                disabled={!isFailed}
                className={cn(
                  "w-2.5 h-2.5 rounded-sm",
                  STATUS_COLOR[status],
                  isFailed && "cursor-pointer hover:ring-1 hover:ring-red-300",
                  !isFailed && "cursor-default",
                )}
              />
            )
          })}
        </div>
      </div>

      {expanded && (
        <div className="px-4 pb-3.5 pt-2 border-t border-border space-y-3">
          {thumbNames.length > 0 && (
            <div className="flex gap-1.5 overflow-x-auto pb-1">
              {thumbNames.map((name) => (
                <HoverCard key={name} openDelay={100} closeDelay={0}>
                  <HoverCardTrigger asChild>
                    <img
                      src={api.workspaces.thumbUrl(ws.slug, name)}
                      alt={name}
                      className="h-16 rounded-md object-cover bg-muted ring-1 ring-border flex-shrink-0 cursor-zoom-in transition-transform hover:scale-[1.02]"
                      loading="lazy"
                    />
                  </HoverCardTrigger>
                  <HoverCardContent
                    side="top"
                    align="center"
                    className="w-auto p-1.5 shadow-2xl"
                  >
                    <img
                      src={api.workspaces.thumbUrl(ws.slug, name)}
                      alt={name}
                      className="max-w-[480px] max-h-[360px] rounded"
                    />
                    <div className="px-2 py-1 text-xs text-muted-foreground font-mono">
                      {name}
                    </div>
                  </HoverCardContent>
                </HoverCard>
              ))}
            </div>
          )}
          <div className="flex gap-2 flex-wrap">
            <Button size="sm" onClick={() => api.workspaces.run(ws.slug, {})}>
              Run
            </Button>
            <Button size="sm" variant="outline" onClick={() => {
              const stage = window.prompt("Reset from which stage?", "detect")
              if (stage) api.workspaces.reset(ws.slug, { from: stage })
            }}>
              Reset…
            </Button>
            <Button size="sm" variant="destructive" onClick={() => {
              if (window.confirm(`Delete workspace ${ws.slug}?`)) {
                api.workspaces.clean(ws.slug)
              }
            }}>
              Clean
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!previewReady}
              onClick={() => api.workspaces.openPreview(ws.slug)
                .catch((e) => alert(`Failed to open preview: ${e.message ?? e}`))}
            >
              Open preview ↗
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
```

Note: the standalone **Approve** button is intentionally removed. Approval now happens through `Save & continue` in the Cuts drawer.

- [ ] **Step 3: Build to verify**

```bash
cd src/vsplit/dashboard/src
bun run build
```

Expected: build completes with no TS errors.

- [ ] **Step 4: Commit (source only, dist in next task)**

```bash
git add src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx src/vsplit/dashboard/src/src/App.tsx
git commit -m "feat(dashboard-fe): Next-action pill on row + wire CutsDrawer"
```

---

## Task 7: Build `dist/`, run full test suite, manual verification

**Files:**
- Modify: `src/vsplit/dashboard/dist/**` (committed build output)

- [ ] **Step 1: Production build**

```bash
cd src/vsplit/dashboard/src
bun run build
```

Expected: `vite build` finishes; `../dist/` contains updated `index.html` + `assets/`.

- [ ] **Step 2: Run full Python test suite**

```bash
cd D:/workspace/auto-video-spliter
uv run pytest -q
```

Expected: `146 passed` (140 existing + 6 from `test_dashboard_api_cuts.py`; the 4 GET tests from Task 1 + 6 PUT tests from Task 2 = 10 new, so total **150 passed**). Adjust the expected number if the existing suite count drifted.

If any existing test fails, STOP and investigate — the change should be additive only.

- [ ] **Step 3: Manual verification on the iPhone-MOV workspace**

This is required because the project has zero frontend tests today.

```bash
# In one terminal:
cd D:/workspace/auto-video-spliter
uv run vsplit -w "D:/Old_E_Drive/iphone_share/mini" reset img-1752-3 --from review
uv run vsplit -w "D:/Old_E_Drive/iphone_share/mini" dashboard --no-open
```

In a browser, open `http://127.0.0.1:8765` and verify the following manually:

1. The `img-1752-3` row shows a **"Run: review →"** primary pill (because review is pending, not yet awaiting).
2. Click the pill. Watch it cycle to **"Running: review…"** with a spinner.
3. Once review hits `awaiting_user`, the pill shows **"Review & approve →"** in amber.
4. Click the amber pill. The Cuts drawer opens, loads the scenes with thumbnails.
5. Uncheck `keep` on scene #2. Set label "test" on scene #1.
6. Click **Save** — drawer stays open, dirty resets (Save button greys out).
7. Open the file at `D:/Old_E_Drive/iphone_share/mini/.vsplit/workspaces/img-1752-3/decisions/cuts.json` in Notepad. Confirm scene #2 has `"keep": false` and scene #1 has `"label": "test"`.
8. Click **Save & continue**. Drawer closes. Pill cycles through `"Run: plan →"` / `"Running: plan…"` → … → **"Done"**.
9. Check `output/` — files exist, scene #2 is NOT in the outputs (because keep=false).
10. Concurrent-edit test: reset to `--from review` again, open the drawer, then in Notepad change the file and save it. Click **Save** in the drawer. A red error banner appears: "cuts.json changed on disk". Drawer stays open.

If any of these fails, fix and re-test. Do not proceed until all 10 pass.

- [ ] **Step 4: Commit the built dist + bump version**

```bash
# After successful manual verification:
git add src/vsplit/dashboard/dist/
git commit -m "build(dashboard): rebuild dist with cuts editor + Next pill"
```

Also bump the version in both `pyproject.toml` and `src/vsplit/__init__.py` from `0.2.1` → `0.3.0`:

Edit `pyproject.toml`:
```toml
version = "0.3.0"
```

Edit `src/vsplit/__init__.py`:
```python
__version__ = "0.3.0"
```

```bash
git add pyproject.toml src/vsplit/__init__.py
git commit -m "release: vsplit v0.3.0"
git tag v0.3.0
```

- [ ] **Step 5: Update HANDOFF.md**

Edit `HANDOFF.md` to:
- Change `Status: v0.2.1 — dashboard polished.` to `Status: v0.3.0 — cuts editor.`
- Add a `v0.3.0 (cuts editor)` section under "Notable bug fixes worth remembering" listing: inline cuts.json editor, Next-action pill, removal of standalone Approve button.
- Move item #1 (inline cuts.json editor) from "Future work" to "What's built".

```bash
git add HANDOFF.md
git commit -m "docs: refresh HANDOFF for v0.3.0"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Implementing task |
|---|---|
| §1 Goal — close the two UX gaps | Tasks 4, 5, 6 |
| §2 Architecture — no new processes | All tasks |
| §3 Components & code layout | Tasks 1, 2 (backend), 3, 4, 5, 6 (frontend) |
| §4.1 GET /cuts | Task 1 |
| §4.2 PUT /cuts (validation, 409, atomic write) | Task 2 |
| §4.3 Tests | Tasks 1 + 2 (10 tests total) |
| §5 Pill state machine | Task 3 (`nextAction.ts`) + Task 6 (`NextPill`) |
| §6.1 Drawer open/load | Task 4 |
| §6.2 Scene rows (keep/label/merge UI, last-scene disabled) | Task 4 |
| §6.3 Dirty tracking + busy disable | Task 4 |
| §6.4 Footer (Cancel/Save/Save & continue) | Tasks 4 + 5 |
| §6.5 No drawer-internal SSE; main page refetches | Task 6 (`refetch` on `onClose`) |
| §7 WorkspaceRow changes (pill in + Approve removed; BatchActionBar unchanged) | Task 6 |
| §8 Walkthroughs | Verified by Task 7 manual steps |
| §9 Out of scope (timestamps frozen) | Task 4 (UI doesn't expose start/end editors) |
| §10 Verification | Task 7 |
| §11 Compatibility (additive; version bump to 0.3.0) | Task 7 |

**Placeholder scan:** none — every step contains exact paths, code, and commands.

**Type consistency:**
- `CutsBody` defined in `api.ts` (Task 3), consumed by `CutsDrawer.tsx` (Task 4) and `nextAction.ts` consumes `Workspace` (also from `api.ts`). Consistent.
- `nextAction` return type `NextAction` exported from `lib/nextAction.ts` (Task 3), consumed by `NextPill` in `WorkspaceRow.tsx` (Task 6). Consistent.
- `onOpenCuts` prop added to `WorkspaceRow`'s `Props` (Task 6) and passed from `App.tsx` (Task 6 step 1). Consistent.
- Backend: `_CutsPutBody.expected_hash` is `str | None`, matching the frontend's `expected_hash: string | null`. Consistent.
