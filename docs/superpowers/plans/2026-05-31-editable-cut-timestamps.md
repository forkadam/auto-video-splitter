# Editable Cut Timestamps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each scene's `start`/`end` editable in the dashboard review drawer, with `HH:MM:SS.fff` + `start < end` validation and a live keyframe-snapped "actual cut point" hint.

**Architecture:** `GET /cuts` returns the workspace's keyframe list; the React drawer reimplements the backend's `snap_preceding` in a small pure helper module and recomputes the snapped start live as the user types. Validation gates the Save buttons. No backend cut logic changes — `PUT /cuts` already accepts arbitrary `start`/`end`.

**Tech Stack:** Python 3 / FastAPI / pytest (run via `uv`); React 19 + TypeScript + Vite, built with `bun`.

**Branch:** `feat/editable-cut-timestamps` (already created; spec committed there).

**Spec:** `docs/superpowers/specs/2026-05-31-editable-cut-timestamps-design.md`

---

## File Structure

- **Modify** `src/vsplit/dashboard/api/workspaces.py` — `get_cuts` returns a sorted `keyframes` list.
- **Modify** `tests/unit/test_dashboard_api_cuts.py` — assert keyframes returned / empty when missing.
- **Create** `src/vsplit/dashboard/src/src/lib/timecode.ts` — pure helpers: `parseTc`, `formatTc`, `snapPreceding`, `TC_RE`.
- **Modify** `src/vsplit/dashboard/src/src/lib/api.ts` — add `keyframes` to `CutsResponse`.
- **Modify** `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx` — editable inputs, validation, snap hint, button gating.
- **Regenerate + commit** `src/vsplit/dashboard/dist/**` — `bun run build` output (a committed artifact guarded by `test_dashboard_dist_present.py`).

---

## Task 1: Backend — return keyframes from `GET /cuts`

**Files:**
- Modify: `src/vsplit/dashboard/api/workspaces.py` (imports + `get_cuts`, ~line 14 and ~line 183-188)
- Test: `tests/unit/test_dashboard_api_cuts.py`

- [ ] **Step 1: Write the failing tests**

Append to `tests/unit/test_dashboard_api_cuts.py`:

```python
def test_get_cuts_includes_sorted_keyframes(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])
    (ws / "probe").mkdir(parents=True, exist_ok=True)
    # Out of order on disk; the API must return them ascending.
    (ws / "probe" / "keyframes.csv").write_text("2.000\n0.000\n4.500\n")
    client = TestClient(create_app(tmp_path))
    body = client.get("/api/workspaces/alpha/cuts").json()
    assert body["keyframes"] == [0.0, 2.0, 4.5]


def test_get_cuts_keyframes_empty_when_csv_missing(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1])  # no probe/keyframes.csv written
    client = TestClient(create_app(tmp_path))
    body = client.get("/api/workspaces/alpha/cuts").json()
    assert body["keyframes"] == []
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py::test_get_cuts_includes_sorted_keyframes tests/unit/test_dashboard_api_cuts.py::test_get_cuts_keyframes_empty_when_csv_missing -v`
Expected: FAIL — `KeyError: 'keyframes'` (response has no `keyframes` key).

- [ ] **Step 3: Add the keyframes import**

In `src/vsplit/dashboard/api/workspaces.py`, add this import alongside the other `vsplit.core` imports (after line 13, `from vsplit.core.paths import workspace_dir`):

```python
from vsplit.core.keyframes import load_keyframes
```

- [ ] **Step 4: Return keyframes from `get_cuts`**

In `get_cuts`, replace the return block (currently):

```python
    scene_ids = [s.id for s in parsed.scenes]
    return {
        "cuts": json.loads(raw),
        "hash": content_hash(cuts_path),
        "scene_thumbs": _scene_thumbs(root, slug, scene_ids),
    }
```

with:

```python
    scene_ids = [s.id for s in parsed.scenes]
    kf_path = workspace_dir(root, slug) / "probe" / "keyframes.csv"
    keyframes = sorted(load_keyframes(kf_path)) if kf_path.is_file() else []
    return {
        "cuts": json.loads(raw),
        "hash": content_hash(cuts_path),
        "scene_thumbs": _scene_thumbs(root, slug, scene_ids),
        "keyframes": keyframes,
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `uv run pytest tests/unit/test_dashboard_api_cuts.py -v`
Expected: PASS — all tests in the file (existing + 2 new) green.

- [ ] **Step 6: Commit**

```bash
git add src/vsplit/dashboard/api/workspaces.py tests/unit/test_dashboard_api_cuts.py
git commit -m "feat(dashboard): return keyframes list from GET /cuts"
```

---

## Task 2: Frontend — timecode helpers + API type

**Files:**
- Create: `src/vsplit/dashboard/src/src/lib/timecode.ts`
- Modify: `src/vsplit/dashboard/src/src/lib/api.ts` (`CutsResponse` type, ~line 36-40)

- [ ] **Step 1: Create the timecode helper module**

Create `src/vsplit/dashboard/src/src/lib/timecode.ts`:

```ts
/** Timecode helpers mirroring vsplit's HH:MM:SS.fff format and keyframe snapping. */

/** Matches the backend _TS_RE in schemas/cuts.py exactly. */
export const TC_RE = /^\d{2}:\d{2}:\d{2}\.\d{3}$/

/** "HH:MM:SS.fff" -> seconds. Assumes the string already matches TC_RE. */
export function parseTc(tc: string): number {
  const [h, m, s] = tc.split(":")
  return Number(h) * 3600 + Number(m) * 60 + Number(s)
}

/** seconds -> "HH:MM:SS.fff" (zero-padded), mirroring the stages' _format_tc. */
export function formatTc(seconds: number): string {
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = seconds - h * 3600 - m * 60
  const ss = s.toFixed(3).padStart(6, "0")
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${ss}`
}

/** Largest keyframe <= t, or 0 if none. Mirrors core/keyframes.snap_preceding. */
export function snapPreceding(t: number, keyframes: number[]): number {
  let best = 0
  for (const k of keyframes) {
    if (k <= t && k > best) best = k
  }
  return best
}
```

- [ ] **Step 2: Add `keyframes` to the `CutsResponse` type**

In `src/vsplit/dashboard/src/src/lib/api.ts`, change the `CutsResponse` type from:

```ts
export type CutsResponse = {
  cuts: CutsBody
  hash: string
  scene_thumbs: Record<string, string>
}
```

to:

```ts
export type CutsResponse = {
  cuts: CutsBody
  hash: string
  scene_thumbs: Record<string, string>
  keyframes: number[]
}
```

- [ ] **Step 3: Typecheck the build**

Run: `cd src/vsplit/dashboard/src && bun run build`
Expected: build succeeds ("✓ built in …"). `tsc` (run first by the build script) reports no type errors. (This regenerates `../dist`; do NOT stage `dist/` in this task — Task 4 owns the dist commit.)

- [ ] **Step 4: Commit (source only)**

```bash
git add src/vsplit/dashboard/src/src/lib/timecode.ts src/vsplit/dashboard/src/src/lib/api.ts
git commit -m "feat(dashboard): timecode helpers + keyframes on CutsResponse"
```

---

## Task 3: Frontend — editable timestamp fields in CutsDrawer

**Files:**
- Modify: `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`

- [ ] **Step 1: Import the timecode helpers**

In `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`, add after the existing `api` import (line 8, `import { api, type CutsBody } from "@/lib/api"`):

```ts
import { TC_RE, parseTc, formatTc, snapPreceding } from "@/lib/timecode"
```

- [ ] **Step 2: Add `keyframes` to the `Loaded` type**

Change the `Loaded` type (lines 19-23) from:

```ts
type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
}
```

to:

```ts
type Loaded = {
  cuts: CutsBody
  hash: string
  thumbs: Record<string, string>
  keyframes: number[]
}
```

- [ ] **Step 3: Carry keyframes through load and save**

In the load effect's `.then` (line 43), change:

```ts
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs })
```

to:

```ts
        setLoaded({ cuts: r.cuts, hash: r.hash, thumbs: r.scene_thumbs, keyframes: r.keyframes })
```

In `save()` (line 104), change:

```ts
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs })
```

to:

```ts
      setLoaded({ cuts: structuredClone(draft), hash: r.hash, thumbs: loaded.thumbs, keyframes: loaded.keyframes })
```

- [ ] **Step 4: Add per-scene validation memos**

Insert these two memos immediately after the `toggleKeepAll` declaration (after line 79, before `const update = ...`):

```ts
  const sceneErrors = useMemo<Record<number, string | null>>(() => {
    const errs: Record<number, string | null> = {}
    for (const s of draft?.scenes ?? []) {
      if (!TC_RE.test(s.start) || !TC_RE.test(s.end)) {
        errs[s.id] = "timestamps must be HH:MM:SS.fff"
      } else if (parseTc(s.start) >= parseTc(s.end)) {
        errs[s.id] = "end must be after start"
      } else {
        errs[s.id] = null
      }
    }
    return errs
  }, [draft])

  const hasErrors = useMemo(
    () => Object.values(sceneErrors).some(Boolean),
    [sceneErrors],
  )
```

(`useMemo` is already imported on line 1.)

- [ ] **Step 5: Replace the scene-row body with editable fields + snap hint**

Replace the entire `.map` callback body (lines 189-232, from `{slug && loaded && draft && draft.scenes.map((s, i) => {` through its closing `})}`) with:

```tsx
          {slug && loaded && draft && draft.scenes.map((s, i) => {
            const thumb = loaded.thumbs[String(s.id)]
            const isLast = i === draft.scenes.length - 1
            const startOk = TC_RE.test(s.start)
            const endOk = TC_RE.test(s.end)
            const snapped = startOk && loaded.keyframes.length > 0
              ? snapPreceding(parseTc(s.start), loaded.keyframes)
              : null
            const drift = snapped !== null ? parseTc(s.start) - snapped : null
            const err = sceneErrors[s.id]
            return (
              <div key={s.id} className="flex gap-3 p-2 rounded-md border border-border bg-card">
                {thumb ? (
                  <img
                    src={api.workspaces.thumbUrl(slug, thumb)}
                    alt=""
                    className="w-32 h-[4.5rem] rounded object-cover bg-muted ring-1 ring-border flex-shrink-0"
                    loading="lazy"
                  />
                ) : (
                  <div className="w-32 h-[4.5rem] rounded bg-muted ring-1 ring-border flex-shrink-0" />
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 font-mono text-xs text-muted-foreground">
                    <span className="shrink-0">#{s.id.toString().padStart(3, "0")}</span>
                    <Input
                      value={s.start}
                      aria-label={`scene ${s.id} start`}
                      onChange={(e) => update(i, { start: e.target.value })}
                      className={`h-7 w-32 text-xs font-mono ${startOk ? "" : "border-red-500/60"}`}
                    />
                    <span>–</span>
                    <Input
                      value={s.end}
                      aria-label={`scene ${s.id} end`}
                      onChange={(e) => update(i, { end: e.target.value })}
                      className={`h-7 w-32 text-xs font-mono ${endOk ? "" : "border-red-500/60"}`}
                    />
                    {drift !== null && (
                      <span className="shrink-0 text-muted-foreground/70">
                        {drift < 0.001
                          ? "⤷ on keyframe"
                          : `⤷ cuts at ${formatTc(snapped!)} (−${drift.toFixed(2)}s)`}
                      </span>
                    )}
                  </div>
                  {err && <div className="mt-1 text-xs text-red-400">{err}</div>}
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
```

- [ ] **Step 6: Gate the Save buttons on validity**

Replace the footer buttons (lines 235-243) from:

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

to:

```tsx
        <DrawerFooter className="flex-row justify-end gap-2">
          <Button variant="ghost" onClick={cancel} disabled={busy}>Cancel</Button>
          <Button variant="secondary" onClick={save} disabled={busy || !isDirty || hasErrors}>
            Save
          </Button>
          <Button onClick={saveAndContinue} disabled={busy || hasErrors}>
            Save & continue
          </Button>
        </DrawerFooter>
```

- [ ] **Step 7: Typecheck + lint**

Run: `cd src/vsplit/dashboard/src && bun run build && bun run lint`
Expected: build succeeds (no `tsc` errors), `eslint .` reports no errors. (This regenerates `../dist`; the dist commit happens in Task 4.)

- [ ] **Step 8: Commit (source only)**

```bash
git add src/vsplit/dashboard/src/src/views/CutsDrawer.tsx
git commit -m "feat(dashboard): editable start/end fields with validation + keyframe hint"
```

---

## Task 4: Rebuild dist, full verification, manual smoke test

**Files:**
- Regenerate + commit: `src/vsplit/dashboard/dist/**`

- [ ] **Step 1: Rebuild the committed dist from final source**

Run: `cd src/vsplit/dashboard/src && bun run build`
Expected: build succeeds; `src/vsplit/dashboard/dist/` updated with new hashed `assets/index-*.js`.

- [ ] **Step 2: Run the dist-presence + cuts API tests**

Run: `uv run pytest tests/unit/test_dashboard_dist_present.py tests/unit/test_dashboard_api_cuts.py -v`
Expected: PASS — `test_dist_index_references_existing_asset` confirms the committed `index.html` points at an asset that exists on disk.

- [ ] **Step 3: Run the full suite to catch regressions**

Run: `uv run pytest -q`
Expected: PASS (no failures introduced; the dashboard, plan, split, and review suites stay green).

- [ ] **Step 4: Manual smoke test in the dashboard**

There is no automated UI test (the frontend has no JS test runner), so verify by hand. Start the dashboard against a working folder that has at least one workspace whose `review` stage has produced `decisions/cuts.json` and whose `probe/keyframes.csv` exists:

Run: `uv run vsplit dashboard <working-folder>` (open the printed URL).

Confirm:
- Open a workspace's "review cuts.json" drawer. The `start`/`end` show as editable inputs.
- Type an invalid timestamp (e.g. `1:2:3`) → the field border turns red, an inline "timestamps must be HH:MM:SS.fff" error appears, and **Save** / **Save & continue** are disabled.
- Set `end` earlier than `start` → inline "end must be after start"; buttons disabled.
- Fix to a valid range → error clears, buttons enable.
- Edit `start` and watch the `⤷ cuts at … (−Δs)` hint update live; set `start` exactly to a keyframe value → hint reads `⤷ on keyframe`.
- Save, reload the drawer, and confirm the edited timestamps persisted to `decisions/cuts.json`.

- [ ] **Step 5: Commit the rebuilt dist**

```bash
git add src/vsplit/dashboard/dist
git commit -m "chore(dashboard): rebuild dist for editable cut timestamps"
```

- [ ] **Step 6: Confirm a clean tree**

Run: `git status --short`
Expected: empty (the two pre-existing untracked `docs/vsplit-*.md` files may remain — they are out of scope for this branch and should be left untouched).

---

## Notes for the implementer

- **`uv` for all Python:** run pytest as `uv run pytest …`, never bare `pytest`/`python`.
- **`snapPreceding` must match the backend:** largest keyframe ≤ `t`, else `0`. The plan stage uses the same rule for `actual_start`; the `end` is **not** snapped (`actual_end = end`), which is why only the `start` field shows a hint.
- **Validation scope is intentional:** `start < end` is enforced on *every* row, including dropped (`keep: false`) ones. If a hand-edited file already has an inverted range, the drawer will flag it on load and block save until fixed — this is desired (it surfaces a real problem).
- **dist is a build artifact but committed:** always rebuild and commit `dist/` after frontend source changes, or `test_dashboard_dist_present.py` will fail.
