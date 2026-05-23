# vsplit dashboard — design

**Status:** approved 2026-05-23 · builds on vsplit v1.0.1+
**Authors:** brainstorm session, this date
**Companion plan:** TBD (to be created by writing-plans skill after this spec is approved)

---

## 1. Goal

A localhost web UI that complements the existing CLI. Lets a single user:

1. **See every workspace at a glance** — slug, source filename, per-stage statuses, thumbnail.
2. **Trigger commands per workspace or in bulk** — `run`, `reset`, `clean`, `approve`. Same semantics as the CLI.
3. **Read failed-stage logs inline** without leaving the page.
4. **Edit `.vsplit/config.yaml`** through a form for common fields plus a collapsible raw-YAML pane for everything else.

Out of scope for v1 (see §10).

---

## 2. Architecture

`vsplit dashboard` is a new CLI subcommand that starts a localhost-only FastAPI server and opens the browser to it.

```
┌─────────────────────────────────────────────────────────────┐
│  CLI:  uv run vsplit dashboard [--port 8765] [--no-open]    │
│                                                              │
│  ┌───────────────┐   localhost only   ┌──────────────────┐  │
│  │   Browser     │ ◄──── HTTP/SSE ──► │  FastAPI server  │  │
│  │  (React SPA)  │                    │ (in vsplit proc) │  │
│  └───────────────┘                    └────────┬─────────┘  │
│                                                │            │
│                                                │ direct     │
│                                                ▼ calls      │
│                                       ┌──────────────────┐  │
│                                       │  vsplit core     │  │
│                                       │  (orchestrator,  │  │
│                                       │   actions, etc.) │  │
│                                       └──────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

- **Bind address:** `127.0.0.1` only. No authentication, no tokens. Single-user assumption.
- **Default port:** 8765. Configurable via `--port`; fails fast if taken.
- **Browser:** Python `webbrowser` opens the user's default browser on startup. `--no-open` skips.
- **Server lifecycle:** tied to the CLI process. Ctrl-C triggers uvicorn's lifespan shutdown — waits up to 5s for in-flight stages, then forces.
- **Frontend:** React + Vite + Tailwind + shadcn/ui, built with **Bun**. Built `dist/` is committed under `src/vsplit/dashboard/dist/`. End users never need Bun installed.
- **Action execution:** server invokes vsplit's Python entrypoints directly — no `subprocess.run("vsplit ...")`. Same process, same imports.
- **Live updates:** Server-Sent Events stream at `/api/events`. Two sources feed it (see §5).

---

## 3. Components & code layout

```
src/vsplit/
  cli/
    dashboard.py            ← new CLI command, registered in cli/__init__.py
  core/
    actions.py              ← NEW (refactor): callable run/reset/clean/approve
  dashboard/
    __init__.py
    server.py               ← FastAPI app + uvicorn launcher
    watcher.py              ← manifest-change watcher thread → SSE events
    sse.py                  ← in-process pub/sub for SSE broadcasting
    api/
      __init__.py
      workspaces.py         ← list/detail/thumbs
      actions.py            ← run/reset/clean/approve (single + batch)
      config.py             ← get/put structured + raw YAML
      logs.py               ← per-stage log retrieval
    src/                    ← Vite/React project root (frontend source)
      package.json          ← bun deps
      vite.config.ts
      tailwind.config.ts
      tsconfig.json
      build.sh              ← one-shot rebuild script for frontend devs
      src/
        main.tsx
        App.tsx
        lib/api.ts          ← typed client (generated from openapi.json)
        lib/sse.ts          ← EventSource hook
        components/ui/      ← shadcn components (Button, Drawer, Switch, …)
        views/
          WorkspaceList.tsx     ← hybrid collapsible list
          WorkspaceRow.tsx      ← collapsed + expanded row
          FilterBar.tsx         ← status/stage/slug filters; selection state
          BatchActionBar.tsx    ← floating bar when ≥1 row checked
          LogDrawer.tsx         ← side drawer for failed-stage logs
          ConfigDrawer.tsx      ← hybrid form + collapsible raw YAML
    dist/                   ← built output, committed to git
```

### Refactor required: `vsplit/core/actions.py`

The existing CLI commands (`run_cmd`, `reset_cmd`, `clean_cmd`, `approve_cmd`) have orchestration logic inline. Extract each into a callable function so the API and CLI hit identical code:

```python
# vsplit/core/actions.py  (new module)
def run_workspaces(working_root: Path, slugs: list[str], *,
                   from_=None, through=None, only=None,
                   concurrency_overrides=None) -> RunResult: ...
def reset_workspaces(working_root: Path, slugs: list[str], *, from_: str) -> None: ...
def clean_workspaces(working_root: Path, slugs: list[str]) -> int: ...
def approve_workspaces(working_root: Path, slugs: list[str]) -> int: ...
```

Each CLI command becomes a thin Click wrapper; each API endpoint a thin FastAPI wrapper.

---

## 4. API surface

All endpoints under `/api/`. JSON request/response unless noted. Errors follow FastAPI's standard `{"detail": ...}` shape.

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/workspaces` | List all: slug, source, status summary, stage statuses, thumb hint |
| `GET` | `/api/workspaces/{slug}` | Full manifest + scene list + thumb filenames |
| `GET` | `/api/workspaces/{slug}/thumbs/{name}` | Serve thumbnail JPG (FileResponse) |
| `POST` | `/api/workspaces/{slug}/run` | Body: `{from?, through?, only?, concurrency_overrides?}` |
| `POST` | `/api/workspaces/{slug}/reset` | Body: `{from}` |
| `POST` | `/api/workspaces/{slug}/clean` | No body; destructive |
| `POST` | `/api/workspaces/{slug}/approve` | No body |
| `GET` | `/api/workspaces/{slug}/logs/{stage}` | Plain-text log file contents |
| `POST` | `/api/batch/run` | Body: `{slugs[], from?, through?, only?}` |
| `POST` | `/api/batch/reset` | Body: `{slugs[], from}` |
| `POST` | `/api/batch/clean` | Body: `{slugs[]}` |
| `POST` | `/api/batch/approve` | Body: `{slugs[]}` |
| `GET` | `/api/config` | Effective config (structured JSON, defaults merged) |
| `PUT` | `/api/config` | Write structured config (server validates via Pydantic) |
| `GET` | `/api/config/raw` | Raw `.vsplit/config.yaml` text (or empty if not present) |
| `PUT` | `/api/config/raw` | Write raw YAML (validated; returns line/col on syntax error) |
| `GET` | `/api/config/schema` | Pydantic JSON-schema (drives client-side form generation) |
| `GET` | `/api/events` | SSE stream |
| `GET` | `/openapi.json` | OpenAPI 3.1 spec (FastAPI built-in) |
| `GET` | `/` and `/{path:path}` | Serves built React SPA from `dashboard/dist/` |

### Non-feature: serving the existing per-workspace `preview/index.html`

The existing preview HTML uses `../../../` relative paths to the source video. Serving it through the dashboard server breaks those paths. Instead, the workspace row's "Open preview ↗" button opens `file://...preview/index.html` in a new tab — same UX as `vsplit one` today. No code in the dashboard server needs to handle that file.

---

## 5. Data flow & SSE

### Status flow into SSE

```
┌──────────────────┐                ┌─────────────────────┐
│  Worker thread   │                │   FastAPI process   │
│  (orchestrator)  │                │                     │
│                  │                │   GET /api/events   │
│  run_stage(...)  │── publish ──►  │   ◄─── SSE ───►     │── browser
│  ↓               │    (queue)     │                     │
│  save_manifest() │                │   broadcasts to     │
│                  │                │   all SSE clients   │
└──────────────────┘                └─────────────────────┘
       │                                       ▲
       │ writes manifest.json                  │
       ▼                                       │
┌──────────────────┐                            │
│ Watcher thread   │── reads manifest mtime ────┘
│  (polls 500ms)   │   emits diff events
└──────────────────┘
```

Two complementary signal sources:

1. **In-process events:** when the dashboard itself dispatches a run, the orchestrator hook wraps `run_stage` and pushes status changes into an `asyncio.Queue` consumed by `/api/events`. Instant updates with no polling.
2. **Watcher thread:** background thread polls every workspace's `manifest.json` mtime every 500ms. Emits `workspace_updated` events when mtime changes. Catches edits from another `uv run vsplit ...` invocation, external editors, or manual JSON edits.

Both paths feed the same SSE broadcaster (the `vsplit/dashboard/sse.py` module). Frontend reducer is unchanged regardless of source.

### Event types

```typescript
type DashboardEvent =
  | { type: "workspace_updated"; slug: string; stages: StageStatusMap; updated_at: string }
  | { type: "workspaces_added"; slugs: string[] }       // scan picked them up
  | { type: "workspaces_removed"; slugs: string[] }     // clean
  | { type: "run_started"; slug: string; stages: string[] }
  | { type: "run_finished"; slug: string; ok: boolean }
  | { type: "config_changed" }                            // dashboard or external write
  | { type: "heartbeat" }                                  // every 15s
```

### EventSource client behavior

- Opens on app mount.
- Auto-reconnects with exponential backoff on disconnect (1s, 2s, 4s, capped at 30s).
- On reconnect, fetches `/api/workspaces` to resync state before resuming the live stream.
- Browser shows a brief "reconnecting…" toast during outage.

---

## 6. Error handling

| Failure mode | Server behavior | UI behavior |
|---|---|---|
| Invalid filter or body | `422 Unprocessable Entity` + Pydantic error list | Inline form error |
| Run requested on non-existent slug | `404` | Toast: "workspace not found, may have been cleaned" |
| Stage throws during dispatch | `200`; failure lands in manifest (`status: failed`, `error: {...}`) | Row turns red, "Logs" button auto-surfaces |
| `PUT /api/config` Pydantic validation fail | `422` + per-field errors | Form highlights bad fields |
| `PUT /api/config/raw` YAML syntax error | `400` + `{line, col, message}` | Error banner, save button stays disabled |
| FFmpeg / PySceneDetect missing at startup | Server still boots; `/api/workspaces` includes `tools_missing: [...]` | Banner at top with install hint |
| SSE connection drops | EventSource reconnects + frontend re-fetches `/api/workspaces` | Brief "reconnecting…" toast |
| User hits Ctrl-C on CLI | uvicorn waits ≤5s for in-flight stages, then shuts down | Browser shows "server disconnected" |
| Concurrent dashboards on same working root | Last writer wins (no locking). Both watchers see each other's manifests; SSE keeps both browsers in sync | Same UI, two browsers see same state |

---

## 7. Testing strategy

### Backend (Python)

```
tests/unit/
  test_dashboard_api_workspaces.py
  test_dashboard_api_actions.py
  test_dashboard_api_config.py
  test_dashboard_api_logs.py
  test_dashboard_watcher.py
  test_dashboard_sse.py
  test_actions.py                  ← new core.actions tests (CLI + API share these)
```

Driver: FastAPI's `TestClient` (sync). For SSE: the test client supports streaming responses — read 2–3 events, assert shape, close.

**Pattern:** each test seeds a `tmp_path` working root with manifest fixtures (using existing `new_manifest`/`save_manifest` helpers), spins up the FastAPI app pointed at that root, exercises endpoint, asserts response and disk side effects.

### Frontend

**v1 stance: minimal frontend testing.**

In scope:
- TypeScript strict mode.
- ESLint + Prettier (standard Vite template).
- One pytest test that asserts `src/vsplit/dashboard/dist/index.html` exists and references an asset file that is also present (catches a forgotten rebuild).

Deferred to v2:
- Vitest unit tests for React hooks.
- Playwright e2e tests.

### Integration

One pytest test exercises the full stack with the existing `tests/fixtures/short.mp4`:

1. Seed a workspace via `adopt_or_find`.
2. `POST /api/workspaces/<slug>/run?through=preview`.
3. Subscribe to SSE in a background thread; collect events until `run_finished`.
4. `GET /api/workspaces/<slug>` → assert `preview.status == "done"`.
5. `POST /api/workspaces/<slug>/clean` → assert workspace dir gone.

---

## 8. Build pipeline

### Developer loop

```bash
# one-time
cd src/vsplit/dashboard/src && bun install

# terminal 1: Vite dev server on :5173 (HMR)
cd src/vsplit/dashboard/src && bun run dev

# terminal 2: FastAPI on :8765, CORS open to :5173
uv run vsplit dashboard --dev
```

In `--dev`, only API calls hit FastAPI; the React app runs from Vite's dev server with hot reload.

### Production build

```bash
cd src/vsplit/dashboard/src
bun run build         # writes ../dist/index.html + ../dist/assets/*
```

The `dist/` folder is **committed to git** under `src/vsplit/dashboard/dist/`. Hatchling's `[tool.hatch.build.targets.wheel]` includes `dist/**`, so installed wheels ship the static assets.

End users never need Bun. Frontend developers do.

### CI guard

A pytest test asserts that `dashboard/dist/index.html` exists and references a `dashboard/dist/assets/*.js` file that is also present on disk. Catches a forgotten rebuild commit.

### TypeScript types from OpenAPI

Type drift between Python (Pydantic) and TS (hand-written) is a known hazard. Mitigation: a single `bun run gen-types` script that:

1. Boots the FastAPI app in a subprocess.
2. Fetches `/openapi.json`.
3. Pipes through `openapi-typescript` to generate `src/lib/api.types.ts`.
4. The typed `api.ts` client imports from that file.

Run before each frontend commit (could become a pre-commit hook later).

---

## 9. Layout: hybrid collapsible list

Selected during brainstorming (`.superpowers/brainstorm/.../content/layout-main.html`):

```
┌──────────────────────────────────────────────────────────────────────────┐
│  vsplit dashboard         D:\vacation\japan-2025 · 96 workspaces  [⚙]    │
├──────────────────────────────────────────────────────────────────────────┤
│  [status: all ▾] [stage: any ▾] [slug: ___________]      3 selected →    │
│                                                          [Run] [Reset]   │
├──────────────────────────────────────────────────────────────────────────┤
│  ▸ ☐  [thumb]  img-1754  ● ● ● ● ● ● ●                                   │
│  ▾ ☑  [thumb]  img-1755  ● ● ● ● ● ● ●                                   │
│        ┌─────────────────────────────────────────────────────────────┐   │
│        │  [scene 1] [scene 2] [scene 3]   ← scene strip from preview │   │
│        │  [Run] [Reset ▾] [Clean] [Open preview ↗] [Logs (none)]     │   │
│        └─────────────────────────────────────────────────────────────┘   │
│  ▸ ☑  [thumb]  img-1756  ● ● 𝗫 ○ ○ ○ ○      [Logs]                       │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Collapsed row** (default): checkbox, 48×28 thumb, slug, 7 stage status pips, contextual quick-action.
- **Expanded row** (click to open): scene-strip thumbs from the preview, full action set including secondary buttons.
- **Filter bar** sticks at top: status filter, stage filter, slug glob input. Mirrors CLI filter grammar.
- **Floating batch action bar** appears at top when ≥1 row is checked; disappears when none.
- **Settings cog** opens the config drawer (form + collapsible raw YAML pane).
- **Failed stage pip** is clickable — opens the log drawer for that stage.
- **Dark theme only.** Matches existing preview HTML.

---

## 10. Out of scope for v1

These are reasonable v1.1+ additions but explicitly excluded:

- **Inline `cuts.json` editor.** Per-scene review UI (keep/drop toggles, label, merge). Big feature; deferred. v1 keeps editing JSON in your editor and opening preview HTML in a new tab.
- **Multi-user / network / auth.** 127.0.0.1 only.
- **Mobile or responsive layouts.** Desktop-optimized (≥1024px).
- **Dark/light theme toggle.** Dark only.
- **Plug-in stages, custom detectors, or any other "sharp v1" scope-creep items.**
- **Live tail of stage logs while running.** Log viewer reads post-failure log file only; SSE streams *status*, not log lines.
- **Undo on destructive actions.** Confirmation dialog only. `clean` and `reset` remain permanent.
- **Watching for new source files in the working root.** Scan is still a CLI-driven step. (Dashboard could trigger `vsplit scan` later; not in v1.)
- **Concurrent multi-user editing of config.** Last-writer-wins; no locking.

---

## 11. Risks

1. **Long-running `dispatch()` blocking the asyncio loop.** Mitigation: every action handler offloads to `fastapi.concurrency.run_in_threadpool`. Orchestrator is already thread-based, so natural.
2. **Bun on Windows.** Bun supports Windows. The build is one-shot manual, not part of `uv pip install`, so user-side install never touches Bun. Frontend devs need it once.
3. **Hatchling include of `dist/`.** Standard; just need the `force-include` glob in `pyproject.toml`.
4. **Pydantic ↔ TypeScript drift.** Mitigation: openapi-typescript codegen.
5. **AV-induced manifest write latency on Windows.** Watcher poll at 500ms should tolerate it; if not, throttle/coalesce. Out-of-process actions (CLI invocations) might lag by up to 1s.
6. **Concurrent dashboards.** Two browsers on the same working root: harmless. Two `vsplit dashboard` processes on the same port: second fails fast.

---

## 12. Open questions to settle during implementation

- Exact shadcn component list (Button, Dialog, Drawer, Switch, Slider, Select, Toast — confirm during build).
- Whether to expose `--concurrency stage=N` overrides through the UI in v1 or save for later. (Lean: not in v1 — config drawer handles it.)
- Per-stage thumbnail strategy when more than ~6 scenes (truncate? horizontal scroll? all of them?). Lean: horizontal scroll, no truncation.
- Whether the workspace row's stage pips are clickable beyond `failed`. Lean: only `failed` → log drawer; others are visual only in v1.
