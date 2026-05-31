# Editable cut timestamps in the dashboard CutsDrawer — Design

> Status: approved 2026-05-31. Companion specs: `docs/vsplit-design-doc.md` (§5.7 human-in-the-loop review).

## 1. Problem

During the review step, a user can manually set the cut time range for any detected
scene by editing `decisions/cuts.json` — the `start`/`end` timestamps there are free-form
(the schema validates only `HH:MM:SS.fff` format and that scene IDs match the detected
set; it does not pin start/end to the detector's output).

The dashboard's review drawer (`CutsDrawer.tsx`) exposes `keep`, `merge_with_next`, and
`label`, but shows `start`/`end` **read-only** (`#003 · 00:00:01.000 – 00:00:02.000`).
So a user who wants to retime a cut from the dashboard has to drop to the JSON file.

**Goal:** make `start`/`end` editable in the drawer, with validation, and surface where
the cut will *actually* land under stream-copy (keyframe-snapped), so the user knows the
cut is not frame-accurate.

## 2. Scope

In scope:
- Editable `start`/`end` per scene in the drawer.
- Client-side validation: `HH:MM:SS.fff` format + `start < end`, blocking save while invalid.
- A live "actual cut point" hint per scene: the keyframe-snapped start + drift.

Explicitly out of scope (YAGNI / preserves existing invariants):
- **No `end` snapping.** The `plan` stage uses `actual_end = end` verbatim
  (`stages/plan.py:67`); only `start` snaps to the preceding keyframe. The hint annotates
  `start` only and is labelled so end-snapping isn't implied.
- **No within-source-duration check.** The drawer has no source duration in hand and the
  plan/split tolerate an end past EOF; not worth the plumbing.
- **No adding brand-new scene rows.** Introducing a time range the detector never produced
  still requires editing `detect/scenes.json` then `reset --from detect` — enforced by the
  existing ID-drift validation in `_validate_ids`. Unchanged.
- **No new JS test framework.** See §7.

## 3. Chosen approach for the "actual cut point" hint

The snapped value must update live as the user edits `start` (otherwise it shows the drift
for the *previous* start and misleads). Three options were considered:

- **A. Ship the keyframe list, compute live in JS (chosen).** `GET /cuts` returns the
  `keyframes` array; the drawer reimplements `snap_preceding` (≈3 lines) in TS and
  recomputes per scene on edit.
- B. Compute server-side on `GET`, show statically — stale the moment `start` is edited.
- C. Debounced `POST /snap` endpoint — chattier, needs debounce + loading state.

**A** is chosen: instant feedback, no extra requests, exactly mirrors the plan stage. The
keyframe list is a few KB for normal GOP video; for all-intra footage it can reach ~1 MB,
but drift there is always ≈0 (a keyframe on every frame) so the payload is harmless, and
this is a localhost dashboard where transfer cost is negligible.

## 4. Backend changes

`src/vsplit/dashboard/api/workspaces.py` — `get_cuts` (`GET /api/workspaces/{slug}/cuts`):

- Add a `keyframes: list[float]` field to the response, loaded via
  `load_keyframes(workspace_dir(root, slug) / "probe" / "keyframes.csv")`, sorted ascending.
- If `keyframes.csv` is absent (shouldn't happen once review has run, since review depends
  on preview → detect → probe, but be defensive), return `[]`. The drawer then hides the
  snap hint.
- Response shape becomes `{cuts, hash, scene_thumbs, keyframes}`.

`PUT /api/workspaces/{slug}/cuts` is **unchanged**: it already accepts arbitrary
`start`/`end` and validates format + IDs server-side (`schemas/cuts.py`, `_validate_ids`).

## 5. Frontend changes

### 5.1 New module `src/lib/timecode.ts` (pure helpers)

- `parseTc(tc: string): number` — `"HH:MM:SS.fff"` → seconds.
- `formatTc(seconds: number): string` — seconds → `"HH:MM:SS.fff"` (zero-padded,
  3-decimal), matching `stages/*._format_tc`.
- `snapPreceding(t: number, keyframes: number[]): number` — largest keyframe ≤ `t`,
  else `0`. Mirrors `core/keyframes.snap_preceding`.
- `TC_RE = /^\d{2}:\d{2}:\d{2}\.\d{3}$/` — mirrors backend `_TS_RE` exactly.

Extracted so `CutsDrawer` stays readable and the logic is unit-testable later.

### 5.2 `src/lib/api.ts`

- Add `keyframes: number[]` to the `CutsResponse` type.

### 5.3 `src/views/CutsDrawer.tsx`

- Replace the read-only `{s.start} – {s.end}` text with two monospace `<Input>` fields,
  wired through the existing `update(i, { start })` / `update(i, { end })`.
- Per-scene validation: a row is invalid if either field fails `TC_RE`, or
  `parseTc(start) >= parseTc(end)`. This applies to **every** scene row, including dropped
  (`keep: false`) ones — matching the backend, which 422s on any bad format. Show an inline
  error line under the offending row.
- Disable **Save** and **Save & continue** whenever any row is invalid (in addition to the
  existing `isDirty` gate on Save).
- Snap hint beside the `start` field, computed live via `snapPreceding(parseTc(start),
  keyframes)` when `start` is valid and `keyframes.length > 0`:
  - drift ≥ 1 ms → `⤷ cuts at 00:01:22.000 (−1.5s)`
  - drift < 1 ms → `⤷ on keyframe`
  - no keyframes → hint hidden.
- `thumbs` state already carried in `Loaded`; add `keyframes` alongside it.

## 6. Data flow

```
GET /cuts ──► { cuts, hash, scene_thumbs, keyframes }
                                   │
                                   ▼
        CutsDrawer draft (editable start/end/keep/merge/label)
                                   │  per keystroke
                  ┌────────────────┴─────────────────┐
                  ▼                                   ▼
         validate (TC_RE + start<end)        snapPreceding(start, keyframes)
                  │                                   │
                  ▼                                   ▼
        Save disabled if invalid            "⤷ cuts at … (−Δs)" hint
                  │
                  ▼  PUT /cuts { cuts, expected_hash }  (format + IDs validated server-side)
            cuts.json on disk ──► plan ──► split (actual_start = snap_preceding(start))
```

## 7. Testing

- **Backend (pytest, TDD):** extend `tests/unit/test_dashboard_api_cuts.py`:
  - Seed a `probe/keyframes.csv`; assert `GET /cuts` returns the parsed `keyframes` list
    (sorted floats).
  - Assert `GET /cuts` returns `keyframes: []` when `keyframes.csv` is absent.
- **Frontend:** the project has no JS test runner configured (only `tsc -b`, `eslint`, and
  the Vite build). This feature does not justify introducing one. Verify via:
  - `tsc -b` typecheck (catches type wiring on `CutsResponse` / helpers),
  - `eslint .`,
  - production build,
  - manual check in the dashboard: edit a timestamp, confirm inline error on bad input and
    start≥end, confirm Save disables, confirm the snap hint tracks edits and reads
    `on keyframe` at a keyframe boundary.
  - The `timecode.ts` helpers are pure, so vitest can be added later with zero refactor if
    desired.

## 8. Risks / tradeoffs accepted

- Keyframe payload size for all-intra footage (~1 MB). Accepted: localhost, and drift ≈0
  there so the data is barely used.
- Validating `start < end` on dropped rows may flag a pre-existing inverted range in a
  hand-edited file on load, blocking save until fixed. Accepted: it surfaces a real problem
  and inverted ranges are never intentional.
- No frontend unit tests for `timecode.ts` in v1. Accepted: pure functions, easy to add
  later; manual + typecheck covers the wiring.
