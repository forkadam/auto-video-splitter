# No-scene Manual Cuts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When detection finds zero scenes, let the user open the cut dialog and either cut the whole clip or enter/remove multiple manual time ranges, then approve & run.

**Architecture:** Relax the cuts ID-validation rule only when the detected set is empty (one shared backend function). `GET /cuts` reports `detected_count` + `duration_seconds`; the React drawer enters a "manual mode" when `detected_count === 0`, adding whole-clip / add-range / remove-row controls on top of the existing editable rows. The "No scenes" pill opens the cut dialog.

**Tech Stack:** Python 3 / FastAPI / pytest (via `uv`); React 19 + TypeScript + Vite, built with `bun`.

**Branch:** `feat/no-scene-manual-cuts` (already created; spec committed there).

**Spec:** `docs/superpowers/specs/2026-05-31-no-scene-manual-cuts-design.md`

---

## File Structure

- **Modify** `src/vsplit/stages/review.py` — `_validate_ids` early-returns when no scenes detected.
- **Modify** `src/vsplit/dashboard/api/workspaces.py` — `get_cuts` returns `detected_count` + `duration_seconds`.
- **Modify** `tests/unit/test_dashboard_api_cuts.py` — cover both backend changes.
- **Modify** `src/vsplit/dashboard/src/src/lib/api.ts` — extend `CutsResponse`.
- **Modify** `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx` — manual mode (add/remove rows, whole-clip, empty state, Save gating).
- **Modify** `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx` — "No scenes" pill opens the cut dialog.
- **Regenerate + commit** `src/vsplit/dashboard/dist/**`.

---

## Task 1: Backend — relax validation + expose detected_count/duration

**Files:**
- Modify: `src/vsplit/stages/review.py` (`_validate_ids`)
- Modify: `src/vsplit/dashboard/api/workspaces.py` (`get_cuts`)
- Test: `tests/unit/test_dashboard_api_cuts.py`

- [ ] **Step 1: Write the failing tests**

In `tests/unit/test_dashboard_api_cuts.py`, change the manifest import line:
```python
from vsplit.core.manifest import new_manifest, save_manifest
```
to:
```python
from vsplit.core.manifest import load_manifest, new_manifest, save_manifest
```
Then append these tests:
```python
def test_get_cuts_returns_detected_count_and_duration(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])
    m = load_manifest(ws)
    m.metadata.duration_seconds = 12.5
    save_manifest(ws, m)
    client = TestClient(create_app(tmp_path))
    body = client.get("/api/workspaces/alpha/cuts").json()
    assert body["detected_count"] == 2
    assert body["duration_seconds"] == 12.5


def test_get_cuts_detected_count_zero_when_no_scenes(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    (ws / "detect").mkdir(parents=True, exist_ok=True)
    (ws / "detect" / "scenes.json").write_text(
        json.dumps({"schema_version": 1, "scenes": []}))
    (ws / "decisions").mkdir(parents=True, exist_ok=True)
    (ws / "decisions" / "cuts.json").write_text(
        json.dumps({"schema_version": 1, "scenes": []}, indent=2) + "\n")
    client = TestClient(create_app(tmp_path))
    body = client.get("/api/workspaces/alpha/cuts").json()
    assert body["detected_count"] == 0


def test_put_cuts_allows_user_scene_when_no_scenes_detected(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    (ws / "detect").mkdir(parents=True, exist_ok=True)
    (ws / "detect" / "scenes.json").write_text(
        json.dumps({"schema_version": 1, "scenes": []}))
    (ws / "decisions").mkdir(parents=True, exist_ok=True)
    (ws / "decisions" / "cuts.json").write_text(
        json.dumps({"schema_version": 1, "scenes": []}, indent=2) + "\n")
    client = TestClient(create_app(tmp_path))
    initial = client.get("/api/workspaces/alpha/cuts").json()
    user_cuts = {
        "schema_version": 1,
        "scenes": [{"id": 1, "start": "00:00:00.000", "end": "00:00:05.000",
                    "keep": True, "label": None, "merge_with_next": False}],
    }
    r = client.put("/api/workspaces/alpha/cuts",
                   json={"cuts": user_cuts, "expected_hash": initial["hash"]})
    assert r.status_code == 200
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -k "detected_count or allows_user_scene" -v`
Expected: FAIL — `KeyError: 'detected_count'` for the GET tests; the PUT test fails with 422 (current `_validate_ids` rejects the extra id).

- [ ] **Step 3: Relax `_validate_ids` when no scenes detected**

In `src/vsplit/stages/review.py`, change the start of `_validate_ids` from:
```python
def _validate_ids(parsed: CutsJson, detected_ids: set[int]) -> None:
    cuts_ids = {s.id for s in parsed.scenes}
```
to:
```python
def _validate_ids(parsed: CutsJson, detected_ids: set[int]) -> None:
    # No scenes detected → the user authors cuts manually (whole-clip or manual
    # ranges), so any scene ids are accepted. With detected scenes present we
    # still keep cuts.json in sync (reject added/removed rows).
    if not detected_ids:
        return
    cuts_ids = {s.id for s in parsed.scenes}
```
Leave the rest of the function (the `missing`/`extra` checks) unchanged.

- [ ] **Step 4: Return `detected_count` + `duration_seconds` from `get_cuts`**

In `src/vsplit/dashboard/api/workspaces.py`, replace the entire `get_cuts` function body with:
```python
@router.get("/{slug}/cuts")
def get_cuts(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    ws = workspace_dir(root, slug)
    cuts_path = ws / "decisions" / "cuts.json"
    if not cuts_path.is_file():
        raise HTTPException(status_code=404, detail="cuts.json not yet written")
    raw = cuts_path.read_text()
    try:
        parsed = CutsJson.model_validate_json(raw)
    except ValidationError as e:
        # include_context=False drops non-JSON-serializable ValueError refs
        # that field_validators put into ctx.
        raise HTTPException(status_code=422, detail=e.errors(include_context=False)) from e
    scene_ids = [s.id for s in parsed.scenes]
    kf_path = ws / "probe" / "keyframes.csv"
    keyframes = sorted(load_keyframes(kf_path)) if kf_path.is_file() else []
    scenes_path = ws / "detect" / "scenes.json"
    detected_count = (
        len(json.loads(scenes_path.read_text()).get("scenes", []))
        if scenes_path.is_file() else 0
    )
    try:
        duration_seconds = load_manifest(ws).metadata.duration_seconds
    except FileNotFoundError:
        duration_seconds = None
    return {
        "cuts": json.loads(raw),
        "hash": content_hash(cuts_path),
        "scene_thumbs": _scene_thumbs(root, slug, scene_ids),
        "keyframes": keyframes,
        "detected_count": detected_count,
        "duration_seconds": duration_seconds,
    }
```
(`load_manifest` and `workspace_dir` are already imported at the top of this file.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -v`
Expected: PASS — all tests in the file, including the 3 new ones. Confirm `test_put_cuts_422_on_id_drift` still PASSES (detected set non-empty → relaxation does not apply).

- [ ] **Step 6: Commit**

```bash
git add src/vsplit/stages/review.py src/vsplit/dashboard/api/workspaces.py tests/unit/test_dashboard_api_cuts.py
git commit -m "feat(cuts): allow user-authored cuts + expose detected_count/duration when no scenes detected"
```

---

## Task 2: Frontend — CutsDrawer manual mode

**Files:**
- Modify: `src/vsplit/dashboard/src/src/lib/api.ts`
- Modify: `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`

Read `CutsDrawer.tsx` first; apply the edits in order.

- [ ] **Step 1: Extend `CutsResponse`**

In `src/vsplit/dashboard/src/src/lib/api.ts`, change:
```ts
export type CutsResponse = {
  cuts: CutsBody
  hash: string
  scene_thumbs: Record<string, string>
  keyframes: number[]
}
```
to:
```ts
export type CutsResponse = {
  cuts: CutsBody
  hash: string
  scene_thumbs: Record<string, string>
  keyframes: number[]
  detected_count: number
  duration_seconds: number | null
}
```

- [ ] **Step 2: Extend the `Loaded` type**

In `CutsDrawer.tsx`, change:
```ts
type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
  keyframes: number[]
}
```
to:
```ts
type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
  keyframes: number[]
  detectedCount: number
  durationSeconds: number | null
}
```

- [ ] **Step 3: Thread the new fields through load + save**

In the load effect's `.then`, change:
```ts
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs, keyframes: r.keyframes })
```
to:
```ts
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs, keyframes: r.keyframes, detectedCount: r.detected_count, durationSeconds: r.duration_seconds })
```
In `save()`, change:
```ts
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs, keyframes: loaded.keyframes })
```
to:
```ts
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs, keyframes: loaded.keyframes, detectedCount: loaded.detectedCount, durationSeconds: loaded.durationSeconds })
```

- [ ] **Step 4: Add the add/remove helpers + derived flags**

Immediately AFTER the `update` function (it ends with `}` after the `setDraft(...)` block) and BEFORE `const cancel = () => {`, insert:
```ts

  const nextSceneId = (scenes: CutsBody["scenes"]) =>
    scenes.length ? Math.max(...scenes.map((s) => s.id)) + 1 : 1

  const addScene = (end: string) => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    next.scenes.push({
      id: nextSceneId(next.scenes),
      start: "00:00:00.000",
      end,
      keep: true,
      label: null,
      merge_with_next: false,
    })
    return next
  })

  const removeScene = (idx: number) => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    next.scenes.splice(idx, 1)
    return next
  })

  // Manual-cut mode: only when detection found nothing. Then the user may add /
  // remove rows and cut the whole clip; otherwise the drawer is read-structure.
  const canEditRows = !!loaded && loaded.detectedCount === 0
  const wholeClipEnd =
    loaded?.durationSeconds != null ? formatTc(loaded.durationSeconds) : null
```
(`formatTc` is already imported; `CutsBody` is already imported.)

- [ ] **Step 5: Add a per-row remove button**

In the scene `.map(...)`, find the end of each row — the content `<div className="flex-1 min-w-0">` closes, then the row's outer `<div>` closes. Change this exact tail:
```tsx
                  </div>
                </div>
              </div>
            )
          })}
```
to:
```tsx
                  </div>
                </div>
                {canEditRows && (
                  <button
                    type="button"
                    onClick={() => removeScene(i)}
                    aria-label={`remove scene ${s.id}`}
                    className="self-start text-muted-foreground hover:text-red-400 leading-none px-1"
                  >
                    ✕
                  </button>
                )}
              </div>
            )
          })}
```

- [ ] **Step 6: Add the empty-state + "Add range" controls**

Immediately AFTER the scene `.map(...)` block and BEFORE the closing `</div>` of the scroll container, i.e. change:
```tsx
          })}
        </div>
```
to:
```tsx
          })}
          {canEditRows && draft && draft.scenes.length === 0 && (
            <div className="text-sm text-muted-foreground space-y-3 py-2">
              <p>No scenes were detected. Cut the whole clip, or add time ranges manually.</p>
              <div className="flex gap-2">
                <Button size="sm" onClick={() => addScene(wholeClipEnd!)} disabled={wholeClipEnd === null}>
                  Cut whole clip
                </Button>
                <Button size="sm" variant="outline" onClick={() => addScene(wholeClipEnd ?? "00:00:00.000")}>
                  + Add range
                </Button>
              </div>
            </div>
          )}
          {canEditRows && draft && draft.scenes.length > 0 && (
            <Button
              size="sm"
              variant="outline"
              className="w-fit"
              onClick={() => addScene(wholeClipEnd ?? "00:00:00.000")}
            >
              + Add range
            </Button>
          )}
        </div>
```
(`Button` is already imported.)

- [ ] **Step 7: Gate "Save & continue" on having something kept**

Change:
```tsx
          <Button onClick={saveAndContinue} disabled={busy || hasErrors}>
            Save & continue
          </Button>
```
to:
```tsx
          <Button onClick={saveAndContinue} disabled={busy || hasErrors || keepCount === 0}>
            Save & continue
          </Button>
```
(`keepCount` is already defined.)

- [ ] **Step 8: Typecheck + lint**

Run: `cd src/vsplit/dashboard/src && bun run build && bun run lint`
Expected: build succeeds (no TS errors). eslint shows the SAME pre-existing problems and NO NEW ones. Do NOT commit `dist/` here (Task 4 owns it).

- [ ] **Step 9: Commit (source only)**

```bash
git add src/vsplit/dashboard/src/src/lib/api.ts src/vsplit/dashboard/src/src/views/CutsDrawer.tsx
git commit -m "feat(dashboard): manual-cut mode in cuts drawer for no-scene workspaces"
```

---

## Task 3: Frontend — "No scenes" pill opens the cut dialog

**Files:**
- Modify: `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`

- [ ] **Step 1: Route the empty-state pill to the cut dialog**

In `WorkspaceRow.tsx`'s `NextPill`, change:
```tsx
    if (a.kind === "empty") return onOpenPreview(ws.slug)
```
to:
```tsx
    if (a.kind === "empty") return onOpenCuts(ws.slug)
```

- [ ] **Step 2: Update the pill tooltip**

Change:
```tsx
            title={a.kind === "empty"
              ? "Detector found 0 scenes — click to open the preview and decide whether to lower the threshold + reset, or clean this workspace"
              : undefined}
```
to:
```tsx
            title={a.kind === "empty"
              ? "Detector found 0 scenes — click to open the cut dialog: cut the whole clip or enter time ranges manually"
              : undefined}
```
(`onOpenCuts` is already a prop of `NextPill`; no other change needed. `onOpenPreview` is still used by the expand panel's "Open preview" button, so it stays imported/used.)

- [ ] **Step 3: Typecheck + lint**

Run: `cd src/vsplit/dashboard/src && bun run build && bun run lint`
Expected: build succeeds; no new eslint violations. (`onOpenPreview` is still referenced elsewhere, so no unused-var error.) Do NOT commit `dist/` here.

- [ ] **Step 4: Commit (source only)**

```bash
git add src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx
git commit -m "feat(dashboard): No-scenes pill opens the cut dialog"
```

---

## Task 4: Rebuild dist, verify, commit

**Files:**
- Regenerate + commit: `src/vsplit/dashboard/dist/**`

- [ ] **Step 1: Rebuild the committed dist from final source**

Run: `cd src/vsplit/dashboard/src && bun run build`
Expected: build succeeds; `../dist/assets/index-*.js` gets a new content hash and `../dist/index.html` references it.

- [ ] **Step 2: Verify**

Run (from repo root `D:\workspace\auto-video-spliter`):
`uv run pytest tests/unit/test_dashboard_dist_present.py -q && uv run pytest -q`
Expected: dist-presence green; full suite green (was 195 passed; now 198 with the 3 new backend tests). If any test FAILS, STOP and report BLOCKED with output — do not commit on a red suite.

- [ ] **Step 3: Manual smoke test (deferred to controller/user)**

A subagent cannot drive a browser. The controller/user verifies in the dashboard against a workspace where detection found 0 scenes:
- The orange "No scenes" pill opens the cut dialog (not the preview).
- Empty state shows "Cut whole clip" (fills `00:00:00.000 → <duration>`) and "+ Add range".
- "+ Add range" adds editable rows; the ✕ removes a row; the timestamp validation + keyframe hint work on manual rows.
- "Save & continue" is disabled until ≥1 valid kept row exists; saving → approve → run produces output clip(s).
- A workspace WITH detected scenes shows NO add/remove/whole-clip controls (unchanged).

- [ ] **Step 4: Commit the rebuilt dist**

```bash
git add src/vsplit/dashboard/dist
git commit -m "chore(dashboard): rebuild dist for no-scene manual cuts"
```

- [ ] **Step 5: Confirm a clean tree**

Run: `git status --short`
Expected: empty EXCEPT the two pre-existing untracked files `docs/vsplit-design-doc.md` and `docs/vsplit-implementation-spec.md` — leave them untouched.

---

## Notes for the implementer

- **Scope guard:** all manual-edit behavior is gated on `detected_count === 0` (frontend `canEditRows`) and the empty-detected early return in `_validate_ids` (backend). Workspaces with detected scenes are unchanged in both layers.
- **`uv` for Python, `bun` for the frontend** (root `src/vsplit/dashboard/src`).
- **New scene ids** use `max(existing)+1` (or 1), so ids stay unique even after removals (`CutsJson` enforces uniqueness).
- **dist is a committed artifact:** rebuild and commit it after frontend changes, or `test_dashboard_dist_present.py` fails.
- LF→CRLF git warnings on Windows are harmless.
