# Merge-all + keep↔merge coupling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bulk "Merge all" control beside "Keep all" in the cuts review drawer, and make the per-scene keep checkbox couple its merge checkbox (both directions), while keeping the bulk Keep-all decoupled from merge.

**Architecture:** All changes live in one React component, `CutsDrawer.tsx`, mirroring the existing `keepCount`/`keepAllState`/`toggleKeepAll` pattern. New peers handle merge: `mergeableCount`, `mergedCount`, `mergeAllState`, `toggleMergeAll`, plus a `setKeep(idx, keep)` handler that implements the keep→merge coupling. No backend changes — `merge_with_next` already exists in the schema and the `plan` stage.

**Tech Stack:** React 19 + TypeScript + Vite, built with `bun`. Python tests run via `uv` (for the dist-presence guard).

**Branch:** `feat/cuts-merge-all` (already created; spec committed there).

**Spec:** `docs/superpowers/specs/2026-05-31-cuts-merge-all-design.md`

---

## File Structure

- **Modify** `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx` — add merge bulk-toggle state/handlers + keep→merge coupling handler; render the "Merge all" checkbox beside "Keep all"; rewire the per-scene keep checkbox.
- **Regenerate + commit** `src/vsplit/dashboard/dist/**` — `bun run build` output (committed artifact, guarded by `test_dashboard_dist_present.py`).

No backend, schema, or test-file changes (the project has no JS test runner; verification is build + lint + manual).

---

## Task 1: Merge-all control + keep↔merge coupling in CutsDrawer

**Files:**
- Modify: `src/vsplit/dashboard/src/src/views/CutsDrawer.tsx`

Read the file first so the edit anchors below match exactly. All four edits are in this one file.

- [ ] **Step 1: Add merge bulk-toggle state/handlers + the `setKeep` coupling handler**

Insert the following immediately AFTER the `toggleKeepAll` definition (it ends with `})` at line 81, just before the blank line preceding `const sceneErrors`):

```ts

  const mergeableCount = Math.max(0, (draft?.scenes.length ?? 0) - 1)
  const mergedCount = useMemo(
    () => draft?.scenes.slice(0, -1).reduce((n, s) => n + (s.merge_with_next ? 1 : 0), 0) ?? 0,
    [draft],
  )
  const mergeAllState: boolean | "indeterminate" = (() => {
    if (mergeableCount === 0) return false
    if (mergedCount === mergeableCount) return true
    if (mergedCount === 0) return false
    return "indeterminate"
  })()
  // Toggle merge on every scene EXCEPT the last (it has no "next"). Never touches keep.
  const toggleMergeAll = () => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    const allMerged = next.scenes.slice(0, -1).every((s) => s.merge_with_next)
    next.scenes.forEach((s, i) => {
      if (i < next.scenes.length - 1) s.merge_with_next = !allMerged
    })
    return next
  })

  // Per-scene keep toggle couples merge both ways: checking keep checks merge,
  // unchecking keep unchecks it. The last scene has no "next", so leave its merge
  // untouched. Merge stays manually overridable via its own checkbox afterward.
  const setKeep = (idx: number, keep: boolean) => setDraft((d) => {
    if (!d) return d
    const next = structuredClone(d)
    const isLast = idx === next.scenes.length - 1
    next.scenes[idx] = {
      ...next.scenes[idx],
      keep,
      merge_with_next: isLast ? next.scenes[idx].merge_with_next : keep,
    }
    return next
  })
```

(`useMemo` and `setDraft` are already in scope.)

- [ ] **Step 2: Render the "Merge all" checkbox beside "Keep all"**

Replace this block (lines 191-206):

```tsx
        {draft && draft.scenes.length > 0 && (
          <label className="mx-4 mb-2 flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none w-fit">
            <Checkbox
              checked={keepAllState}
              onCheckedChange={toggleKeepAll}
              aria-label="Toggle keep on all scenes"
            />
            <span>
              {keepCount === 0
                ? `Keep all (${draft.scenes.length})`
                : keepCount === draft.scenes.length
                ? `Keep none (${draft.scenes.length})`
                : `${keepCount} of ${draft.scenes.length} kept`}
            </span>
          </label>
        )}
```

with:

```tsx
        {draft && draft.scenes.length > 0 && (
          <div className="mx-4 mb-2 flex items-center gap-4 text-xs text-muted-foreground select-none">
            <label className="flex items-center gap-2 cursor-pointer w-fit">
              <Checkbox
                checked={keepAllState}
                onCheckedChange={toggleKeepAll}
                aria-label="Toggle keep on all scenes"
              />
              <span>
                {keepCount === 0
                  ? `Keep all (${draft.scenes.length})`
                  : keepCount === draft.scenes.length
                  ? `Keep none (${draft.scenes.length})`
                  : `${keepCount} of ${draft.scenes.length} kept`}
              </span>
            </label>
            {mergeableCount > 0 && (
              <label className="flex items-center gap-2 cursor-pointer w-fit">
                <Checkbox
                  checked={mergeAllState}
                  onCheckedChange={toggleMergeAll}
                  aria-label="Toggle merge on all scenes"
                />
                <span>
                  {mergedCount === 0
                    ? `Merge all (${mergeableCount})`
                    : mergedCount === mergeableCount
                    ? `Merge none (${mergeableCount})`
                    : `${mergedCount} of ${mergeableCount} merged`}
                </span>
              </label>
            )}
          </div>
        )}
```

- [ ] **Step 3: Rewire the per-scene keep checkbox to the coupling handler**

Replace this block (lines 258-262):

```tsx
                    <label className="flex items-center gap-1.5 text-sm">
                      <Checkbox checked={s.keep}
                                onCheckedChange={(c) => update(i, { keep: !!c })} />
                      keep
                    </label>
```

with:

```tsx
                    <label className="flex items-center gap-1.5 text-sm">
                      <Checkbox checked={s.keep}
                                onCheckedChange={(c) => setKeep(i, !!c)} />
                      keep
                    </label>
```

Leave the merge checkbox immediately below it unchanged — it stays `onCheckedChange={(c) => update(i, { merge_with_next: !!c })}` so merge remains independently toggleable.

- [ ] **Step 4: Typecheck + lint**

Run in the frontend dir:
`cd src/vsplit/dashboard/src && bun run build && bun run lint`
Expected: build succeeds (no TypeScript errors). `eslint .` reports the SAME pre-existing problems as before and NO new ones (the 2 pre-existing `set-state-in-effect` errors in `CutsDrawer.tsx` useEffect bodies are unrelated to this change).
NOTE: the build regenerates `../dist`. Do NOT stage or commit `dist/` in this task — Task 2 owns the dist commit.

- [ ] **Step 5: Commit (source only)**

```bash
git add src/vsplit/dashboard/src/src/views/CutsDrawer.tsx
git commit -m "feat(dashboard): merge-all control + keep-merge coupling in cuts drawer"
```

---

## Task 2: Rebuild dist, verify, commit

**Files:**
- Regenerate + commit: `src/vsplit/dashboard/dist/**`

- [ ] **Step 1: Rebuild the committed dist from final source**

Run: `cd src/vsplit/dashboard/src && bun run build`
Expected: build succeeds; `../dist/assets/index-*.js` gets a new content hash and `../dist/index.html` references it.

- [ ] **Step 2: Run the dist-presence + full suite**

Run (from repo root `D:\workspace\auto-video-spliter`):
`uv run pytest tests/unit/test_dashboard_dist_present.py -q && uv run pytest -q`
Expected: PASS — dist-presence test green (index.html references an asset that exists), and the full suite green (195 passed). If any test FAILS, STOP and report BLOCKED with the output; do not commit on a red suite.

- [ ] **Step 3: Manual smoke test (deferred to controller/user)**

A subagent cannot drive a browser; the controller/user verifies in the dashboard:
- "Merge all" appears beside "Keep all"; toggling it flips every non-last merge box (last stays disabled/unchecked); the tri-state label updates; the control is hidden when a workspace has a single scene.
- Checking a scene's keep also checks its merge (except the last scene); unchecking keep clears its merge; merge can still be toggled manually afterward.
- Clicking bulk "Keep all" / "Keep none" does NOT change any merge box.

Note this step is deferred; do not attempt it as a subagent.

- [ ] **Step 4: Commit the rebuilt dist**

```bash
git add src/vsplit/dashboard/dist
git commit -m "chore(dashboard): rebuild dist for merge-all + keep-merge coupling"
```

- [ ] **Step 5: Confirm a clean tree**

Run: `git status --short`
Expected: empty EXCEPT the two pre-existing untracked files `docs/vsplit-design-doc.md` and `docs/vsplit-implementation-spec.md` — out of scope; leave them untouched.

---

## Notes for the implementer

- **`merge_with_next` semantics (unchanged):** the `plan` stage only merges *adjacent kept* scenes and ignores merge on dropped scenes, so coupling merge to keep is safe — it just makes "keep a run → one stitched clip" the default.
- **Intended asymmetry:** the per-scene keep click couples merge; the bulk Keep-all toggle (`toggleKeepAll`, unchanged) does NOT. `toggleMergeAll` only ever sets `merge_with_next`, never `keep`.
- **Last scene:** never gets `merge_with_next = true` (no "next"). Its merge checkbox is already `disabled={isLast}`; the new logic preserves that invariant.
- **`uv` for Python**, **`bun` for the frontend** (root `src/vsplit/dashboard/src`).
- **dist is a committed build artifact:** always rebuild and commit it after frontend source changes, or `test_dashboard_dist_present.py` fails.
