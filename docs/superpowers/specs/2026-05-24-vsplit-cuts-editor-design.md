# vsplit cuts editor & next-action pill — design

**Status:** approved 2026-05-24 · builds on vsplit v0.2.1
**Authors:** brainstorm session, this date
**Companion plan:** TBD (to be created by writing-plans skill after this spec is approved)

---

## 1. Goal

Close the two biggest UX gaps in the v0.2.1 dashboard:

1. **No editor for `decisions/cuts.json`.** A fresh user clicks Run, sees the amber `awaiting_user` pip, and has no in-app way to know that the gate requires hand-editing a JSON file in `<workspace>/decisions/`. The HANDOFF defers an inline editor as "the v0.3 headline feature."
2. **No "what do I do next?" cue.** The seven status dots have no labels; the collapsed row offers no next-action affordance; users must expand a row and guess which button advances the pipeline.

This spec covers both gaps as one cohesive change: a **Next-action pill** on the collapsed row that, for the `review = awaiting_user` state, opens a **Cuts editor drawer** where the user can flip `keep`, set `label`, and toggle `merge_with_next` per scene, then save-and-continue in one gesture.

Out of scope: see §9.

---

## 2. Architecture

No new processes, no new infra. Two backend endpoints, two frontend additions, one button removal.

```
┌────────────────────────────────────────────────────────────────┐
│  Workspace row (collapsed)                                     │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ▸ ▢ [thumb]  slug             [Next-action pill →]  ●●●●●●● │
│  └──────────────────────────────────────────────────────────┘  │
│                          │                                     │
│                          │ click (review=awaiting_user only)   │
│                          ▼                                     │
│            ┌─────────────────────────────────────┐             │
│            │  Cuts editor drawer                 │             │
│            │  ┌────────────────────────────────┐ │             │
│            │  │ [thumb] #1  0:00.0–0:09.5      │ │             │
│            │  │   [✓] keep   label: [____]     │ │             │
│            │  │   [ ] merge with next          │ │             │
│            │  ├────────────────────────────────┤ │             │
│            │  │ [thumb] #2  …                  │ │             │
│            │  │                                │ │             │
│            │  ├────────────────────────────────┤ │             │
│            │  │ [Cancel] [Save] [Save & continue]│ │             │
│            │  └────────────────────────────────┘ │             │
│            └─────────────────────────────────────┘             │
└────────────────────────────────────────────────────────────────┘
```

- Pill colour encodes urgency (red / amber / blue / primary / muted) and click handler depends on workspace state (see §5).
- Drawer follows the same pattern as `LogDrawer` / `ConfigDrawer` (shadcn `Sheet`).
- Server-side validation reuses the existing `CutsJson` Pydantic model.
- Save uses the existing `atomic_write_text` helper; the new file hash naturally drives the existing `review` re-entry logic — no behavior fork between CLI and dashboard flows.
- Optimistic-concurrency tokens are **freshly hashed file contents** (`content_hash(decisions/cuts.json)`), not the manifest's stored `cuts_file_hash` artifact (which can lag behind a concurrent CLI edit).

---

## 3. Components & code layout

```
src/vsplit/
  dashboard/
    api/
      workspaces.py            ← NEW endpoints: GET/PUT /cuts
    src/src/views/
      WorkspaceRow.tsx         ← add Next pill; remove standalone Approve button
      CutsDrawer.tsx           ← NEW
      App.tsx                  ← wire CutsDrawer open/close state
    src/src/lib/
      api.ts                   ← cuts.get / cuts.put / openCutsDrawer wiring
      nextAction.ts            ← NEW: pure function (workspace state → pill spec)
  schemas/cuts.py              ← unchanged (re-used as-is)
```

No backend file other than `dashboard/api/workspaces.py` is touched. The pipeline, orchestrator, manifest schema, and review stage are all unchanged.

---

## 4. Backend endpoints

### 4.1 `GET /api/workspaces/{slug}/cuts`

**Purpose:** load the current `decisions/cuts.json` plus the per-scene thumbnail filename the editor should render.

**Response 200:**

```json
{
  "cuts": { "schema_version": 1, "scenes": [ … CutsJson scenes … ] },
  "hash": "sha256-hex-of-cuts.json",
  "scene_thumbs": { "1": "scene-001-00.jpg", "2": "scene-002-00.jpg", … }
}
```

- `hash` is `content_hash(decisions/cuts.json)` — same helper the review stage uses.
- `scene_thumbs` maps each scene `id` to the **first** matching thumbnail filename in `preview/images/` (pattern `scene-{id:03d}-*.jpg`, sorted, take first). Missing thumbs are simply absent from the map; the drawer renders a placeholder.

**Errors:**
- 404 if slug unknown (existing `_require_slug` helper).
- 404 if `decisions/cuts.json` does not exist (review hasn't run yet).

### 4.2 `PUT /api/workspaces/{slug}/cuts`

**Purpose:** validate and atomically write `decisions/cuts.json`.

**Request body:**

```json
{
  "cuts": { "schema_version": 1, "scenes": [ … ] },
  "expected_hash": "sha256-hex-or-null"
}
```

**Behavior:**

1. `_require_slug` (404 on unknown).
2. Parse `cuts` via `CutsJson.model_validate` — 422 on schema failure with Pydantic's error list.
3. Run `_validate_ids(parsed, detected_ids)` against `detect/scenes.json` — 422 on mismatch.
4. If `expected_hash` is non-null and `content_hash(decisions/cuts.json) != expected_hash` → 409 Conflict with body `{"detail": "cuts.json changed on disk", "current_hash": "…"}`.
5. Write via `atomic_write_text(cuts_path, json.dumps(parsed.model_dump(), indent=2) + "\n")`.
6. Return `{"hash": <new_hash>}`.

**Side effects:**
- The hash change is what unblocks the next `review` run (per `review.py:84-93`). No additional manifest mutation here — the existing re-entry logic handles it. This keeps the CLI flow (`edit cuts.json by hand + click approve + run`) and the dashboard flow (`save in drawer + click run`) on the **exact same backend path**.

**Errors:**
- 404 slug / 404 cuts not yet written / 409 hash mismatch / 422 schema or ID validation.

### 4.3 Routing & tests

Registered alongside existing routes in `dashboard/api/workspaces.py`. New unit tests added under `tests/unit/test_dashboard_api_cuts.py` covering: happy-path GET, happy-path PUT, 404 missing cuts.json, 409 hash mismatch, 422 schema error, 422 ID drift.

---

## 5. Next-action pill state machine

A pure function in `lib/nextAction.ts` takes the existing `Workspace` summary fields (`stages`, `any_failed`, `any_awaiting`, `all_done`) and returns one of these specs:

| Predicate (evaluated top-down, first match wins) | Pill text | Click handler | Visual |
|---|---|---|---|
| `all_done === true` | `Done` | none | muted/green, no chevron |
| `any_failed === true` (find first stage with `status === "failed"` → `X`) | `Retry: {X} →` | `api.workspaces.run(slug, {only: X})` | red |
| any stage has `status === "running"` (first such stage `X`) | `Running: {X}…` | none, spinner | blue, pulsing |
| `stages.review === "awaiting_user"` | `Review & approve →` | open `CutsDrawer` for this slug | amber |
| else find first stage with `status !== "done"` → `X` | `Run: {X} →` | `api.workspaces.run(slug, {})` | primary |
| (fallthrough — shouldn't trigger) | `Idle` | none | muted |

The function is unit-tested (`tests/unit/cuts-pill.test.ts`? — frontend has no test framework yet; punt to a pure-TS module that can be tested later. **For this iteration, manual verification only**: render every state in a Storybook-style fixture in the dev session and screenshot.).

**Placement:** on the collapsed row, between the slug text and the seven-pip strip. The existing `flex-1` slug column shrinks to accommodate. On viewports < 600px we hide the pill and rely on the expanded row's buttons (existing behaviour).

**Clickable area:** the pill is a `<button>`; its action depends on the spec above. Terminal states (`Done`, `Running…`, `Idle`) render as a non-button `<span>` with no hover affordance.

---

## 6. Cuts editor drawer

### 6.1 Open / load

- Triggered by the Next pill when `stages.review === "awaiting_user"`. Drawer state lives in `App.tsx` (similar to `LogDrawer`): `{slug: string | null}`.
- On open, `GET /api/workspaces/{slug}/cuts`. Show a loading row while pending. On 404 ("cuts not yet written") show an inline error with a Close button — should not happen in practice because we only open from `awaiting_user`, but defensively handled.

### 6.2 Scene rows

For each scene in `cuts.scenes`:

```
┌───────────────────────────────────────────────────────────────┐
│ [thumb 80×45]   #001   00:00:00.000 – 00:00:09.498           │
│                 ┌────────────────────────────────┐            │
│                 │ [✓] keep    label: [_________] │            │
│                 │ [ ] merge with next            │            │
│                 └────────────────────────────────┘            │
└───────────────────────────────────────────────────────────────┘
```

- `id`, `start`, `end` are read-only display. Rendering: `${id.toString().padStart(3,'0')}` and the timestamps as-stored.
- `keep`: shadcn `Checkbox`.
- `label`: `Input` (max 100 chars, soft limit, no server-side max enforced by schema). Empty string is normalised to `null` on save (matches the schema default and the CLI's hand-edit convention).
- `merge_with_next`: shadcn `Checkbox`. The **last scene** in the list has this control disabled and forced to `false` — merging into nothing is meaningless and `CutsJson` doesn't validate this today; the UI prevents the case so we don't need a schema change.

### 6.3 Dirty tracking

- Local state mirrors the loaded cuts. A `useMemo` computes `isDirty` by deep-equality against the original. `Save` is disabled when `!isDirty`. `Save & continue` is always enabled when not mid-flight (it allows the "approve as-is" path).
- All three footer buttons are disabled while any API call is in flight to prevent double-submit.

### 6.4 Footer actions

Footer has three buttons (right-aligned):

| Button | Behavior |
|---|---|
| **Cancel** | Close drawer. If `isDirty`, confirm with native `window.confirm("Discard unsaved changes?")`. |
| **Save** | `PUT /cuts` with `{cuts, expected_hash: loadedHash}`. On 200, update `loadedHash`, reset dirty. On 409, toast `"cuts.json changed on disk"` with a Reload button that re-fetches. On 422, render the Pydantic error message in a banner above the footer. Stays open. |
| **Save & continue** | If `isDirty`: `PUT /cuts`. Then `POST /approve`. Then `POST /run` (no body — advances chain). Close drawer on success. Errors at any step show the same banner-toast pattern; downstream calls are skipped on the first failure. |

The PUT-then-approve-then-run sequence is **three serial API calls from the frontend**, not a new combined backend endpoint. Reasons: each step has different failure semantics already exposed by existing routes; combining them would require either a new transaction-style endpoint (out of scope) or invisible retry logic (worse UX than a clear step-by-step error message).

### 6.5 Close behavior

Drawer manages its own SSE subscription? **No.** It just renders the loaded cuts; the main page's SSE listener handles workspace updates and refetches the workspace list on `workspace_updated` events, which is what drives the pill state to refresh after `Save & continue` triggers downstream stages.

---

## 7. WorkspaceRow changes

Modifications to `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`:

1. **Add** Next-action pill between slug text and pip strip (collapsed row).
2. **Remove** the standalone `Approve` button in the expanded row. Rationale: `Save & continue` in the drawer is now the one approve gesture, and approving without seeing the cuts is no longer a feature we want to encourage. CLI's `vsplit approve` still works for users who want the old flow.
3. **Keep** `Run`, `Reset…`, `Clean`, `Open preview ↗`. The pill is a shortcut for the most-likely next action; the expanded panel remains the place for less-common actions.

`BatchActionBar.tsx`: keep the `Approve` button. Batch-approve is still useful for CI-like "approve all five workspaces" flows; the inline editor is per-workspace.

---

## 8. Data flow walkthroughs

### 8.1 Happy path: first-time review

1. Pipeline runs through `preview`; `review` hits `awaiting_user`.
2. Workspace row's pill renders **"Review & approve →"** (amber).
3. User clicks pill → drawer opens, `GET /cuts` returns initial `keep: true` scenes (per `_initial_cuts` default).
4. User unchecks `keep` on a couple of scenes, sets a label on one, clicks **Save & continue**.
5. Frontend: `PUT /cuts` (200) → `POST /approve` (200) → `POST /run` (200). Drawer closes.
6. Backend: `run_workspaces` → `_unlock_approved_reviews` flips `review` to pending → orchestrator runs `review` (re-entry path, hash changed, proceeds to `done`) → `plan` → `split` → `verify`.
7. SSE `workspace_updated` events drive the row's pill through `"Run: plan →"` (briefly) → `"Running: plan…"` → … → `"Done"`.

### 8.2 Concurrent edit (CLI + dashboard)

1. User opens drawer, `GET /cuts` returns hash `h1`.
2. User runs `notepad <workspace>/decisions/cuts.json` and saves an unrelated change.
3. User clicks **Save** in drawer. `PUT /cuts` includes `expected_hash: h1`.
4. Server: `content_hash` is now `h2`, mismatch → 409 with `current_hash: h2` in the body.
5. Drawer shows toast `"cuts.json changed on disk"` with a Reload button. Reload triggers another `GET /cuts`, replaces local state, drops dirty flag.

### 8.3 Save without continue

1. User edits, clicks **Save** (not "Save & continue") to leave the workspace in `awaiting_user`.
2. `PUT /cuts` (200). Drawer stays open. `loadedHash` updates to the new hash. `isDirty` resets to false. Pill text remains **"Review & approve →"** because `review` is still `awaiting_user`.
3. User can close, reopen later, edit more, then `Save & continue` to advance.

---

## 9. Out of scope (v0.3.0)

- Editing `start` / `end` timestamps. The schema permits it; the UI doesn't expose it. Workaround documented inline in the drawer: `"To re-cut scenes, run vsplit reset --from detect with different thresholds."`
- Adding or removing scenes (the validator forbids ID drift; the only way is `reset --from detect`).
- Drag-reorder scenes (scene order is intrinsic to the source video).
- Visual timeline / waveform scrubbing.
- Auto-save / multi-tab sync.
- Frontend unit tests — the project has zero frontend test coverage today (HANDOFF item). The Next-action function is written as a pure module to make it test-ready when Vitest or similar is introduced.
- Mobile / narrow-viewport UX for the drawer (works on desktop ≥1024px; collapses awkwardly below that — acceptable for a localhost tool).

---

## 10. Verification

1. `uv run pytest -q` — all 140 existing tests still pass; new backend tests (≥6) pass.
2. Manual UI verification on the real iPhone-MOV workspace from `D:\Old_E_Drive\iphone_share\mini`:
   - Reset workspace to `--from review`, click Run.
   - Observe pill cycling `"Run: probe →"` → `"Running: probe…"` → … → `"Review & approve →"`.
   - Open drawer, verify scene rows render with thumbnails, edit, Save → drawer stays open, dirty resets.
   - Save & continue → pill cycles to `"Run: plan →"` → … → `"Done"`. Outputs land in `output/`.
3. Concurrent-edit case (§8.2) verified by editing the file via a text editor while the drawer is open.
4. Pill renders for every state listed in §5 (manually toggle by setting manifest stage statuses in a scratch workspace, refresh).

---

## 11. Compatibility

- No schema migration. `cuts.json` format is unchanged.
- CLI commands unchanged. `vsplit approve` still works for users who prefer the hand-edit flow.
- The new endpoints are additive; old dashboard builds (none exist in the wild — this is single-user, locally-built) wouldn't call them.
- Bumps to `0.3.0` because of the new headline feature; the dashboard rebuild ships under `src/vsplit/dashboard/dist/`.
