# Manual cuts for no-scene workspaces — Design

> Status: approved 2026-05-31. Surfaces: `dashboard/api/workspaces.py`, `stages/review.py`,
> `dashboard/src/src/views/CutsDrawer.tsx`, `lib/api.ts`, `lib/nextAction.ts`,
> `views/WorkspaceRow.tsx`.

## 1. Problem / goal

When scene detection finds **zero scenes**, the workspace dead-ends: `detect` writes
`scenes: []`, `review` writes an empty `cuts.json` and sits `awaiting_user`, and the dashboard
shows an orange **"No scenes"** pill that opens the *preview* pane. There is no way to produce
an output clip — the cuts drawer would be empty, and the `cuts.json` ID-validation rule blocks
adding rows.

Goal: for **no-scene workspaces only**, let the user open the cut dialog and either **cut the
whole clip** or **enter one or more time ranges manually** (each a separate output clip),
edit/remove them, then approve & run.

## 2. Constraints / current invariant being relaxed

`_validate_ids(parsed, detected_ids)` (in `stages/review.py`, also imported by `PUT /cuts` in
`dashboard/api/workspaces.py`) rejects:
- **missing**: a detected scene ID absent from `cuts.json` (don't delete detected rows), and
- **extra**: a `cuts.json` scene ID not in `detect/scenes.json` (don't add rows here).

With zero detected scenes, the "extra" check rejects any manually-added row. We relax this
**only when the detected set is empty**.

## 3. Backend changes

### 3.1 `_validate_ids` — relax when no scenes detected
`stages/review.py`: at the top of `_validate_ids`, if `detected_ids` is empty, return early
(accept any user-authored scenes). When detected scenes exist, the missing/extra checks are
unchanged. This single function is shared by the `review` stage re-entry and `PUT /cuts`, so
both honor the relaxation identically.

### 3.2 `GET /cuts` — return `detected_count` and `duration_seconds`
`dashboard/api/workspaces.py` `get_cuts`: add to the response:
- `detected_count: int` — number of scenes in `detect/scenes.json` (0 if the file is absent).
- `duration_seconds: float | null` — `manifest.metadata.duration_seconds` (load the manifest).

The drawer uses `detected_count === 0` to enable manual mode and `duration_seconds` to fill in
"cut whole clip". No schema change — `CutsJson`/`Scene` already permit arbitrary unique int IDs
and `HH:MM:SS.fff` start/end, and `plan`/`split` already process any kept scenes.

## 4. Frontend changes

### 4.1 Open the dialog from the "No scenes" pill
`views/WorkspaceRow.tsx`: the `NextPill` click for `a.kind === "empty"` changes from
`onOpenPreview(ws.slug)` to `onOpenCuts(ws.slug)`. Update its tooltip to describe manual
cutting. The preview remains reachable via the row's existing "Open preview" button and the
expand panel. `nextAction.ts`'s `empty` kind/label is unchanged (label stays "No scenes").

### 4.2 `lib/api.ts`
Add `detected_count: number` and `duration_seconds: number | null` to `CutsResponse`.

### 4.3 `CutsDrawer.tsx` — manual mode (active only when `detected_count === 0`)
- `Loaded` carries `detectedCount` and `durationSeconds`; threaded through both `setLoaded`
  calls. `canEditRows = loaded.detectedCount === 0`.
- New-row id: `nextId = scenes.length ? max(scene ids) + 1 : 1`. New rows default to
  `{ id, start: "00:00:00.000", end: <duration or "00:00:00.000">, keep: true,
  merge_with_next: false, label: null }`.
- **Empty state** (manual mode AND `scenes.length === 0`): a short explanatory line plus two
  buttons — **"Cut whole clip"** (adds one row spanning `00:00:00.000 → formatTc(duration)`;
  disabled when `durationSeconds == null`) and **"+ Add range"**.
- **With rows** (manual mode): each row shows a **remove (✕)** button; a persistent
  **"+ Add range"** button sits at the bottom of the list.
- Add/remove UI and the empty-state buttons render **only** when `canEditRows`. Workspaces with
  detected scenes (`detected_count > 0`) keep today's drawer exactly.
- The existing editable start/end fields, `HH:MM:SS.fff` + `start < end` validation, keyframe
  hint, keep/merge/label controls, and keep-all/merge-all bulk toggles all apply to manual
  rows unchanged.

### 4.4 Save gating
Disable **"Save & continue"** when `keepCount === 0` (in addition to the existing `busy` /
`hasErrors` gates). This covers the empty state (nothing added yet) and is universally correct
— approving with nothing kept produces no output.

## 5. Data flow

```
No-scenes workspace (review awaiting_user, scenes_found 0)
   │  click "No scenes" pill → onOpenCuts
   ▼
GET /cuts → { cuts:{scenes:[]}, hash, scene_thumbs, keyframes,
              detected_count: 0, duration_seconds }
   │  detected_count === 0 → manual mode
   ▼
user: "Cut whole clip" or "+ Add range" (×N), edit start/end, remove rows
   │  Save → PUT /cuts  (relaxed _validate_ids accepts user IDs since detected set empty)
   ▼
Save & continue → approve + run → review re-entry (relaxed validation) → plan → split → clips
```

## 6. Edge cases
- `duration_seconds == null` (probe missing it): "Cut whole clip" disabled; "+ Add range"
  defaults end to `00:00:00.000`, which fails `start < end` and surfaces the existing inline
  error, prompting the user to type a real end.
- Removing all rows then saving → `cuts.json` `scenes: []`; "Save & continue" is disabled
  (`keepCount === 0`), so the workspace simply returns to the no-output state. No crash.
- Non-empty workspaces (`detected_count > 0`): no add/remove UI; `_validate_ids` still enforces
  the ID match; behavior unchanged.

## 7. Testing
- **Backend (pytest)** — extend `tests/unit/test_dashboard_api_cuts.py`:
  - `GET /cuts` returns `detected_count` (matching `detect/scenes.json`) and `duration_seconds`.
  - `PUT /cuts` with a user-added scene id **succeeds (200)** when `detect/scenes.json` has zero
    scenes; still **422** on id drift when detect has scenes (existing test stays green).
  - Optionally a direct `_validate_ids` unit check for the empty-detected early return.
- **Frontend** — no JS test runner; verify via `bun run build` (typecheck), `eslint` (no new
  violations), and a manual smoke test:
  - A no-scene workspace's "No scenes" pill opens the cut dialog; empty state shows the two
    buttons; "Cut whole clip" fills a full-span row; "+ Add range" adds editable rows; remove
    works; Save & continue is disabled until ≥1 kept valid row exists; save → approve → run
    produces output.
  - A workspace WITH detected scenes shows no add/remove UI (unchanged).
- Rebuild and commit `dist/` (guarded by `test_dashboard_dist_present.py`).
