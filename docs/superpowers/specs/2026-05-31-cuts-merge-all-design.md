# Merge-all control + keep↔merge coupling in CutsDrawer — Design

> Status: approved 2026-05-31. Surface: `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`.

## 1. Problem / goal

The review drawer has a bulk "Keep all / Keep none" tri-state checkbox and per-scene
`keep` / `merge_with_next` / `label` controls. Users who keep a run of scenes usually want
those scenes stitched into a single output clip, which means setting `merge_with_next` on
each kept scene — currently a per-scene chore.

Add two conveniences:
1. A bulk **"Merge all"** control alongside "Keep all".
2. Clicking a scene's **keep** checkbox also flips its **merge** checkbox, so "keep a run →
   one stitched clip" is the default, while merge stays manually overridable.

## 2. Background (existing semantics, unchanged)

`merge_with_next` only bridges **adjacent kept** scenes in `plan.py` (a kept scene `N` merges
into `N+1` only when both are kept and consecutive); it is ignored on dropped scenes, and the
**last scene has no "next"** so its merge box is disabled. These rules are unchanged — this
is purely a dialog-interaction convenience over the same data.

## 3. Behavior

### 3.1 "Merge all" bulk control
- Rendered in the same header row as "Keep all", to its right.
- Operates over *mergeable* scenes = **all scenes except the last**. Let `M = scenes.length − 1`.
- Tri-state, mirroring keep-all:
  - none merged → unchecked, label `Merge all (M)`
  - all mergeable merged → checked, label `Merge none (M)`
  - partial → indeterminate, label `X of M merged`
- `toggleMergeAll`: if every non-last scene already has `merge_with_next`, clear it on all
  non-last scenes; otherwise set it on all non-last scenes. The **last scene's
  `merge_with_next` is never set** (stays `false`/disabled). Never touches `keep`.
- Hidden entirely when `scenes.length <= 1` (nothing to merge).

### 3.2 Per-scene keep → merge coupling
The keep checkbox uses a dedicated handler (not the generic `update`). Unified rule:
`merge_with_next = isLast ? (unchanged) : keep`.
- **Check keep** (keep=true) → set `merge_with_next = true`, except the last scene (left as-is,
  i.e. stays `false`).
- **Uncheck keep** (keep=false) → set `merge_with_next = false` (last scene left as-is, already
  `false` — a no-op).
- The merge checkbox keeps its own independent `onChange` (`update(i, { merge_with_next })`),
  so after a keep click the user can still toggle merge manually.

### 3.3 Intended asymmetry (confirmed)
- The **per-scene keep** click couples merge (both directions).
- The **bulk "Keep all/none"** toggle does **NOT** touch merge — it only flips `keep`.
- "Merge all" and "Keep all" are otherwise independent bulk toggles, each owning one dimension.

## 4. Out of scope
- No backend / schema / `plan` changes.
- Bulk keep-all does not imply merge-all (explicitly rejected).
- No change to validation, the editable timestamp fields, or the keyframe hint.

## 5. Implementation notes
- All new logic lives inline in `CutsDrawer.tsx`, consistent with the existing
  `keepCount` / `keepAllState` / `toggleKeepAll`. New peers: `mergeableCount`,
  `mergeAllState`, `toggleMergeAll`, and a `setKeep(idx, keep)` handler implementing the
  coupling. Considered extracting pure reducer helpers for unit-testing but rejected (tight
  coupling to `draft` state, small surface — YAGNI).
- The last-scene guard already exists for the per-scene merge checkbox (`disabled={isLast}`);
  the new logic reuses the same "last scene = not mergeable" rule.

## 6. Testing
- No JS test runner in the project (per prior specs). Verify via `bun run build` (typecheck),
  `bun run lint` (no NEW violations), and a manual smoke test in the dashboard:
  - "Merge all" toggles every non-last merge box; the last stays disabled/unchecked; tri-state
    label updates; control hidden with a single scene.
  - Checking a scene's keep checks its merge (except the last); unchecking keep clears merge;
    merge still manually toggleable afterward.
  - Bulk "Keep all" leaves merge boxes untouched.
- Rebuild and commit `dist/` (guarded by `test_dashboard_dist_present.py`).
