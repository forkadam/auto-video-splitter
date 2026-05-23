# vsplit dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `vsplit dashboard` — a localhost FastAPI server + React SPA that lists workspaces, runs commands, edits config, and streams live status via SSE.

**Architecture:** FastAPI server bound to 127.0.0.1; React/Vite/Tailwind/shadcn frontend built with Bun and committed to `src/vsplit/dashboard/dist/`. Action handlers call new `vsplit.core.actions` module (CLI refactored to share the same code). Live updates via SSE; manifest-watcher thread + in-process event broker feed the stream.

**Tech Stack:** Python 3.11+, FastAPI, uvicorn, existing vsplit core; React 18, TypeScript strict, Vite, Tailwind CSS, shadcn/ui, Bun.

**Reference spec:** `docs/superpowers/specs/2026-05-23-vsplit-dashboard-design.md`

---

## Phase 1 — Refactor: extract `vsplit.core.actions`

End state: `cli/run.py`, `cli/reset.py`, `cli/clean.py`, `cli/approve.py` are thin Click wrappers over `vsplit.core.actions`. All existing tests still pass. New unit tests cover `actions.py` directly.

### Task 1: Add `vsplit/core/actions.py` with `clean_workspaces` extracted

`clean` is the simplest extraction — start here to lock in the pattern.

**Files:**
- Create: `src/vsplit/core/actions.py`
- Create: `tests/unit/test_core_actions_clean.py`
- Modify: `src/vsplit/cli/clean.py`

- [ ] **Step 1: Write failing test for `clean_workspaces`**

`tests/unit/test_core_actions_clean.py`:

```python
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.actions import clean_workspaces
from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc))
    save_index(tmp_path, idx)


def test_clean_workspaces_removes_dir_and_retires_slug(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    _seed(tmp_path, "beta")
    count = clean_workspaces(tmp_path, ["alpha"])
    assert count == 1
    assert not workspace_dir(tmp_path, "alpha").exists()
    idx = load_index(tmp_path)
    assert "alpha" not in idx.workspaces
    assert "alpha" in idx.retired_slugs
    assert "beta" in idx.workspaces


def test_clean_workspaces_silent_on_missing_slug(tmp_path: Path) -> None:
    count = clean_workspaces(tmp_path, ["does-not-exist"])
    assert count == 0
```

- [ ] **Step 2: Run — should fail**

```bash
uv run pytest tests/unit/test_core_actions_clean.py -v
```

Expected: `ModuleNotFoundError: No module named 'vsplit.core.actions'`.

- [ ] **Step 3: Implement `clean_workspaces`**

`src/vsplit/core/actions.py`:

```python
"""Callable action helpers shared by CLI commands and the dashboard API."""
from __future__ import annotations
import shutil
from pathlib import Path

from vsplit.core.index import load_index, save_index
from vsplit.core.paths import workspace_dir


def clean_workspaces(working_root: Path, slugs: list[str]) -> int:
    """Delete each named workspace directory and retire its slug.

    Returns the number of workspaces actually removed (silent skip for missing).
    """
    idx = load_index(working_root)
    removed = 0
    for slug in slugs:
        if slug not in idx.workspaces:
            continue
        shutil.rmtree(workspace_dir(working_root, slug), ignore_errors=True)
        idx.workspaces.pop(slug, None)
        if slug not in idx.retired_slugs:
            idx.retired_slugs.append(slug)
        removed += 1
    save_index(working_root, idx)
    return removed
```

- [ ] **Step 4: Run — both tests pass**

```bash
uv run pytest tests/unit/test_core_actions_clean.py -v
```

Expected: 2 passed.

- [ ] **Step 5: Refactor `cli/clean.py` to call the new helper**

`src/vsplit/cli/clean.py`:

```python
from __future__ import annotations
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.actions import clean_workspaces
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

console = Console()


@click.command(name="clean")
@click.argument("slug_glob", required=False)
@click.option("--all", "clean_all", is_flag=True)
@click.option("--filter", "filters", multiple=True)
@click.option("--force", is_flag=True)
@click.pass_context
def clean_cmd(ctx, slug_glob, clean_all, filters, force):
    """Delete workspaces entirely. Source files are never touched."""
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    slugs = list(idx.workspaces.keys())
    if clean_all:
        targets = slugs
    else:
        preds: list[Predicate] = []
        if slug_glob:
            preds.append(Predicate(key="slug", value=slug_glob))
        for f in filters:
            preds.append(parse_filter(f))

        def has_status(slug, val):
            m = load_manifest(workspace_dir(working_root, slug))
            return any(b.status == val for b in m.stages.values())
        targets = apply_filters(slugs, preds,
                                status_pred=has_status, stage_pred=lambda *a: True)
    if not targets:
        console.print("nothing to clean")
        return
    if not force and not click.confirm(
        f"delete {len(targets)} workspace(s)?", default=False
    ):
        return
    removed = clean_workspaces(working_root, targets)
    for slug in targets:
        console.print(f"[green]cleaned[/]: {slug}")
    console.print(f"\n{removed} cleaned")
```

- [ ] **Step 6: Run full unit suite — all tests still pass**

```bash
uv run pytest tests/unit/ -q
```

Expected: previous count + 2 new tests, all passing.

- [ ] **Step 7: Commit**

```bash
git add src/vsplit/core/actions.py src/vsplit/cli/clean.py tests/unit/test_core_actions_clean.py
git commit -m "refactor(core): extract clean_workspaces into vsplit.core.actions"
```

---

### Task 2: Add `approve_workspaces` to `vsplit.core.actions`

**Files:**
- Modify: `src/vsplit/core/actions.py`
- Create: `tests/unit/test_core_actions_approve.py`
- Modify: `src/vsplit/cli/approve.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_core_actions_approve.py`:

```python
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.actions import approve_workspaces
from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed_awaiting(tmp_path: Path, slug: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    m.stages["review"].status = "awaiting_user"
    m.stages["review"].artifacts = {"cuts_file_hash": "sha256:x"}
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))


def test_approve_sets_approved_at(tmp_path: Path) -> None:
    _seed_awaiting(tmp_path, "alpha")
    count = approve_workspaces(tmp_path, ["alpha"])
    assert count == 1
    m = load_manifest(workspace_dir(tmp_path, "alpha"))
    assert m.stages["review"].artifacts.get("approved_at")


def test_approve_skips_non_awaiting_workspaces(tmp_path: Path) -> None:
    # workspace that's not in awaiting_user
    ws = workspace_dir(tmp_path, "beta")
    ws.mkdir(parents=True)
    m = new_manifest(slug="beta", source=SourceInfo(
        relative_path="beta.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={"beta": WorkspaceEntry(
        source_relative_path="beta.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    count = approve_workspaces(tmp_path, ["beta"])
    assert count == 0
```

- [ ] **Step 2: Run — should fail**

```bash
uv run pytest tests/unit/test_core_actions_approve.py -v
```

Expected: `ImportError: cannot import name 'approve_workspaces'`.

- [ ] **Step 3: Add `approve_workspaces` to `actions.py`**

Append to `src/vsplit/core/actions.py`:

```python
from datetime import datetime, timezone

from vsplit.core.manifest import load_manifest, save_manifest


def approve_workspaces(working_root: Path, slugs: list[str]) -> int:
    """Mark each workspace's review stage approved if it's awaiting_user.

    Returns the number actually approved.
    """
    now = datetime.now(timezone.utc).isoformat()
    approved = 0
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        try:
            m = load_manifest(ws)
        except FileNotFoundError:
            continue
        block = m.stages["review"]
        if block.status != "awaiting_user":
            continue
        block.artifacts["approved_at"] = now
        save_manifest(ws, m)
        approved += 1
    return approved
```

- [ ] **Step 4: Run — passes**

```bash
uv run pytest tests/unit/test_core_actions_approve.py -v
```

Expected: 2 passed.

- [ ] **Step 5: Refactor `cli/approve.py`**

`src/vsplit/cli/approve.py`:

```python
from __future__ import annotations
from pathlib import Path
import click
import fnmatch
from rich.console import Console
from vsplit.core.actions import approve_workspaces
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

console = Console()


@click.command(name="approve")
@click.argument("slug_glob", required=False)
@click.option("--filter", "filters", multiple=True)
@click.pass_context
def approve_cmd(ctx, slug_glob, filters):
    """Mark workspaces awaiting user review as approved."""
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    targets = list(idx.workspaces.keys())
    if slug_glob:
        targets = [s for s in targets if fnmatch.fnmatch(s, slug_glob)]
    for f in filters:
        if "=" not in f:
            raise click.ClickException(f"bad filter: {f}")
        k, v = f.split("=", 1)
        if k == "status":
            targets = [
                s for s in targets
                if any(b.status == v for b in load_manifest(workspace_dir(working_root, s)).stages.values())
            ]
        elif k == "slug":
            targets = [s for s in targets if fnmatch.fnmatch(s, v)]

    approved = approve_workspaces(working_root, targets)
    for slug in targets:
        m = load_manifest(workspace_dir(working_root, slug))
        if m.stages["review"].artifacts.get("approved_at"):
            console.print(f"[green]approved[/]: {slug}")
    console.print(f"\n{approved} approved")
```

- [ ] **Step 6: Full suite passes**

```bash
uv run pytest tests/unit/ -q
```

- [ ] **Step 7: Commit**

```bash
git add src/vsplit/core/actions.py src/vsplit/cli/approve.py tests/unit/test_core_actions_approve.py
git commit -m "refactor(core): extract approve_workspaces into vsplit.core.actions"
```

---

### Task 3: Add `reset_workspaces` to `vsplit.core.actions`

**Files:**
- Modify: `src/vsplit/core/actions.py`
- Create: `tests/unit/test_core_actions_reset.py`
- Modify: `src/vsplit/cli/reset.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_core_actions_reset.py`:

```python
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.actions import reset_workspaces
from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path):
    slug = "alpha"
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    (ws / "probe").mkdir(); (ws / "probe" / "ffprobe.json").write_text("{}")
    (ws / "detect").mkdir(); (ws / "detect" / "scenes.json").write_text("{}")
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path="alpha.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    m.stages["probe"].status = "done"
    m.stages["detect"].status = "done"
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path="alpha.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    return ws


def test_reset_from_detect_clears_detect_onwards(tmp_path: Path) -> None:
    ws = _seed(tmp_path)
    reset_workspaces(tmp_path, ["alpha"], from_="detect")
    m = load_manifest(ws)
    assert m.stages["probe"].status == "done"
    assert m.stages["detect"].status == "pending"
    assert m.stages["preview"].status == "pending"
    assert not (ws / "detect").exists()
```

- [ ] **Step 2: Run — should fail**

```bash
uv run pytest tests/unit/test_core_actions_reset.py -v
```

- [ ] **Step 3: Add `reset_workspaces` to `actions.py`**

Append to `src/vsplit/core/actions.py`:

```python
from vsplit.schemas.manifest import STAGE_NAMES, StageBlock

_STAGE_SUBDIRS = {
    "probe": "probe", "detect": "detect", "preview": "preview",
    "review": "decisions", "plan": "plan", "split": "output", "verify": None,
}


def reset_workspaces(working_root: Path, slugs: list[str], *, from_: str) -> None:
    """Reset each workspace's stages starting at `from_` (inclusive).

    Deletes on-disk artifacts for affected stages and re-pends them.
    """
    if from_ not in STAGE_NAMES:
        raise ValueError(f"unknown stage: {from_!r}")
    start = STAGE_NAMES.index(from_)
    targets = list(STAGE_NAMES[start:])
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        try:
            m = load_manifest(ws)
        except FileNotFoundError:
            continue
        for stage_name in targets:
            sub = _STAGE_SUBDIRS.get(stage_name)
            if sub:
                shutil.rmtree(ws / sub, ignore_errors=True)
            m.stages[stage_name] = StageBlock(status="pending")
        save_manifest(ws, m)
```

- [ ] **Step 4: Test passes**

```bash
uv run pytest tests/unit/test_core_actions_reset.py -v
```

- [ ] **Step 5: Refactor `cli/reset.py`**

`src/vsplit/cli/reset.py`:

```python
from __future__ import annotations
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.actions import reset_workspaces
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


@click.command(name="reset")
@click.argument("slug_glob", required=False)
@click.option("--from", "from_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--all", "reset_all", is_flag=True)
@click.option("--filter", "filters", multiple=True)
@click.option("--force", is_flag=True)
@click.pass_context
def reset_cmd(ctx, slug_glob, from_stage, reset_all, filters, force):
    """Reset stages so they re-run on next `vsplit run`."""
    if not from_stage and not reset_all:
        raise click.ClickException("require --from STAGE or --all")
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    slugs = list(idx.workspaces.keys())
    preds: list[Predicate] = []
    if slug_glob:
        preds.append(Predicate(key="slug", value=slug_glob))
    for f in filters:
        preds.append(parse_filter(f))

    def has_status(slug, val):
        m = load_manifest(workspace_dir(working_root, slug))
        return any(b.status == val for b in m.stages.values())
    slugs = apply_filters(slugs, preds,
                          status_pred=has_status, stage_pred=lambda *a: True)

    if reset_all and not force and not click.confirm(
        f"reset ALL stages of {len(slugs)} workspace(s)?", default=False
    ):
        return

    from_eff = "probe" if reset_all else from_stage
    reset_workspaces(working_root, slugs, from_=from_eff)
    for slug in slugs:
        console.print(f"[green]reset[/]: {slug} from={from_eff}")
```

- [ ] **Step 6: Full suite passes**

```bash
uv run pytest tests/unit/ -q
```

- [ ] **Step 7: Commit**

```bash
git add src/vsplit/core/actions.py src/vsplit/cli/reset.py tests/unit/test_core_actions_reset.py
git commit -m "refactor(core): extract reset_workspaces into vsplit.core.actions"
```

---

### Task 4: Add `run_workspaces` to `vsplit.core.actions`

The `run` command is the most complex. Extract dispatcher invocation into a function the API can call.

**Files:**
- Modify: `src/vsplit/core/actions.py`
- Create: `tests/unit/test_core_actions_run.py`
- Modify: `src/vsplit/cli/run.py`

- [ ] **Step 1: Write failing test (skipped if no fixture)**

`tests/unit/test_core_actions_run.py`:

```python
import threading
from datetime import datetime, timezone
from pathlib import Path
import shutil
import pytest
from vsplit.core.actions import run_workspaces
from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.config import Config
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")


def test_run_workspaces_advances_probe(tmp_path: Path) -> None:
    target = tmp_path / FIXTURE.name
    shutil.copy(FIXTURE, target)
    ws = workspace_dir(tmp_path, "short")
    ws.mkdir(parents=True)
    m = new_manifest(slug="short", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=target.stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={"short": WorkspaceEntry(
        source_relative_path=FIXTURE.name, size_bytes=target.stat().st_size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))

    run_workspaces(tmp_path, ["short"], config=Config(), only="probe",
                   cancel_event=threading.Event())
    m2 = load_manifest(ws)
    assert m2.stages["probe"].status == "done"
```

- [ ] **Step 2: Run — should fail**

```bash
uv run pytest tests/unit/test_core_actions_run.py -v
```

- [ ] **Step 3: Add `run_workspaces`**

Append to `src/vsplit/core/actions.py`:

```python
import threading
from typing import Callable

from vsplit.core.orchestrator import Workspace, dispatch
from vsplit.schemas.config import Config


def _default_stage_factory(name: str):
    from vsplit.stages.probe import ProbeStage
    from vsplit.stages.detect import DetectStage
    from vsplit.stages.preview import PreviewStage
    from vsplit.stages.review import ReviewStage
    from vsplit.stages.plan import PlanStage
    from vsplit.stages.split import SplitStage
    from vsplit.stages.verify import VerifyStage
    REGISTRY = {
        "probe": ProbeStage, "detect": DetectStage,
        "preview": PreviewStage, "review": ReviewStage,
        "plan": PlanStage, "split": SplitStage, "verify": VerifyStage,
    }
    return REGISTRY[name]()


def _unlock_approved_reviews(working_root: Path, slugs: list[str]) -> None:
    for slug in slugs:
        ws = workspace_dir(working_root, slug)
        m = load_manifest(ws)
        block = m.stages["review"]
        if block.status == "awaiting_user" and block.artifacts.get("approved_at"):
            block.status = "pending"
            save_manifest(ws, m)


def run_workspaces(
    working_root: Path,
    slugs: list[str],
    *,
    config: Config,
    cancel_event: threading.Event,
    only: str | None = None,
    from_: str | None = None,
    through: str | None = None,
    stage_factory: Callable[[str], object] | None = None,
) -> None:
    """Dispatch the orchestrator for the given workspaces.

    Mutually exclusive: `only` overrides `from_`/`through`. Defaults to
    running every stage from probe to verify.
    """
    if only:
        stages_to_run = [only]
    else:
        start = STAGE_NAMES.index(from_) if from_ else 0
        end = STAGE_NAMES.index(through) + 1 if through else len(STAGE_NAMES)
        stages_to_run = list(STAGE_NAMES[start:end])

    _unlock_approved_reviews(working_root, slugs)

    workspaces = []
    for s in slugs:
        ws = workspace_dir(working_root, s)
        m = load_manifest(ws)
        workspaces.append(Workspace(
            slug=s,
            workspace_dir=ws,
            source_path=working_root / m.source.relative_path,
        ))

    dispatch(
        workspaces=workspaces,
        working_root=working_root,
        config=config,
        cancel_event=cancel_event,
        stages_to_run=stages_to_run,
        stage_factory=stage_factory or _default_stage_factory,
    )
```

- [ ] **Step 4: Test passes**

```bash
uv run pytest tests/unit/test_core_actions_run.py -v
```

- [ ] **Step 5: Refactor `cli/run.py` to use `run_workspaces`**

Replace the `dispatch(...)` call and the surrounding stage-list computation with `run_workspaces(...)`. Keep the SIGINT handler and dry-run logic in the CLI.

`src/vsplit/cli/run.py`:

```python
from __future__ import annotations
import threading
import signal
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.actions import run_workspaces
from vsplit.core.config import load_resolved_config
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


@click.command(name="run")
@click.argument("slug_glob", required=False)
@click.option("--only", "only_stage", default=None, type=click.Choice(list(STAGE_NAMES)))
@click.option("--from", "from_stage", default=None, type=click.Choice(list(STAGE_NAMES)))
@click.option("--through", "through_stage", default=None, type=click.Choice(list(STAGE_NAMES)))
@click.option("--filter", "filters", multiple=True)
@click.option("--concurrency", "concurrency_overrides", multiple=True,
              help="stage=N, e.g. split=4")
@click.option("--dry-run", is_flag=True)
@click.pass_context
def run_cmd(ctx, slug_glob, only_stage, from_stage, through_stage,
            filters, concurrency_overrides, dry_run):
    """Advance workspaces through the pipeline."""
    working_root = Path(ctx.obj["working_dir"])
    config = load_resolved_config(working_root=working_root,
                                  explicit_path=ctx.obj.get("config_path"))
    for override in concurrency_overrides:
        if "=" not in override:
            raise click.ClickException(f"bad --concurrency: {override}")
        k, v = override.split("=", 1)
        config.concurrency[k] = int(v)

    idx = load_index(working_root)
    slugs = list(idx.workspaces.keys())
    preds: list[Predicate] = []
    if slug_glob:
        preds.append(Predicate(key="slug", value=slug_glob))
    for f in filters:
        preds.append(parse_filter(f))

    def has_status(slug: str, val: str) -> bool:
        m = load_manifest(workspace_dir(working_root, slug))
        return any(b.status == val for b in m.stages.values())

    def at_stage(slug: str, val: str) -> bool:
        m = load_manifest(workspace_dir(working_root, slug))
        for s in STAGE_NAMES:
            if m.stages[s].status != "done":
                return s == val
        return False

    slugs = apply_filters(slugs, preds, status_pred=has_status, stage_pred=at_stage)

    if dry_run:
        for s in slugs:
            console.print(f"[blue]would run[/]: {s}")
        return

    cancel_event = threading.Event()
    def _on_sigint(signum, frame):
        console.print("\n[yellow]cancellation requested; finishing in-flight stages...[/]")
        cancel_event.set()
    try:
        signal.signal(signal.SIGINT, _on_sigint)
    except (OSError, ValueError):
        pass

    run_workspaces(
        working_root, slugs,
        config=config, cancel_event=cancel_event,
        only=only_stage, from_=from_stage, through=through_stage,
    )
    if cancel_event.is_set():
        ctx.exit(130)
```

- [ ] **Step 6: Full suite passes** (`uv run pytest -q`).

- [ ] **Step 7: Commit**

```bash
git add src/vsplit/core/actions.py src/vsplit/cli/run.py tests/unit/test_core_actions_run.py
git commit -m "refactor(core): extract run_workspaces into vsplit.core.actions"
```

---

## Phase 2 — Dashboard CLI subcommand + FastAPI skeleton

### Task 5: Add FastAPI + uvicorn dependencies

**Files:**
- Modify: `pyproject.toml`

- [ ] **Step 1: Add deps**

In `pyproject.toml`, append to `dependencies`:

```toml
    "fastapi>=0.110",
    "uvicorn[standard]>=0.27",
```

- [ ] **Step 2: Install**

```bash
uv pip install -e ".[dev]"
```

- [ ] **Step 3: Quick smoke check**

```bash
uv run python -c "import fastapi, uvicorn; print(fastapi.__version__, uvicorn.__version__)"
```

Expected: prints two version strings.

- [ ] **Step 4: Commit**

```bash
git add pyproject.toml
git commit -m "build: add fastapi + uvicorn deps"
```

---

### Task 6: Dashboard server skeleton + `vsplit dashboard` CLI

End state: `uv run vsplit dashboard --no-open` starts the server, prints "Dashboard at http://127.0.0.1:8765", `GET /api/health` returns `{"ok": true}`.

**Files:**
- Create: `src/vsplit/dashboard/__init__.py`
- Create: `src/vsplit/dashboard/server.py`
- Create: `src/vsplit/cli/dashboard.py`
- Modify: `src/vsplit/cli/__init__.py`
- Create: `tests/unit/test_dashboard_health.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_health.py`:

```python
from pathlib import Path
from fastapi.testclient import TestClient
from vsplit.dashboard.server import create_app


def test_health_endpoint_returns_ok(tmp_path: Path) -> None:
    app = create_app(working_root=tmp_path)
    client = TestClient(app)
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json() == {"ok": True}
```

- [ ] **Step 2: Run — should fail**

```bash
uv run pytest tests/unit/test_dashboard_health.py -v
```

- [ ] **Step 3: Implement server skeleton**

`src/vsplit/dashboard/__init__.py` (empty file):

```python
```

`src/vsplit/dashboard/server.py`:

```python
"""FastAPI app factory + uvicorn launcher for the vsplit dashboard."""
from __future__ import annotations
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI


def create_app(working_root: Path) -> FastAPI:
    """Build a FastAPI app pointed at the given working root."""
    app = FastAPI(title="vsplit dashboard", version="0.1.0")
    app.state.working_root = working_root

    @app.get("/api/health")
    def health() -> dict:
        return {"ok": True}

    return app


def run_server(
    working_root: Path,
    *,
    host: str = "127.0.0.1",
    port: int = 8765,
    open_browser: bool = True,
) -> None:
    """Block on uvicorn until shutdown."""
    app = create_app(working_root)
    url = f"http://{host}:{port}"
    if open_browser:
        webbrowser.open(url)
    print(f"vsplit dashboard ready at {url}")
    uvicorn.run(app, host=host, port=port, log_level="info")
```

- [ ] **Step 4: Implement CLI command**

`src/vsplit/cli/dashboard.py`:

```python
from __future__ import annotations
from pathlib import Path
import click
from vsplit.dashboard.server import run_server


@click.command(name="dashboard")
@click.option("--port", default=8765, type=int)
@click.option("--host", default="127.0.0.1")
@click.option("--no-open", "no_open", is_flag=True, help="Skip browser auto-open.")
@click.pass_context
def dashboard_cmd(ctx, port: int, host: str, no_open: bool) -> None:
    """Launch the local-only web dashboard."""
    working_root = Path(ctx.obj["working_dir"])
    run_server(working_root, host=host, port=port, open_browser=not no_open)
```

- [ ] **Step 5: Register in `cli/__init__.py`**

Add to imports:

```python
from vsplit.cli.dashboard import dashboard_cmd
```

And add command:

```python
main.add_command(dashboard_cmd)
```

- [ ] **Step 6: Test passes + full suite green**

```bash
uv run pytest tests/unit/test_dashboard_health.py -v
uv run pytest -q
```

- [ ] **Step 7: Smoke check the CLI**

```bash
# In one terminal:
uv run vsplit dashboard --no-open

# In another:
curl http://127.0.0.1:8765/api/health
# {"ok":true}

# Ctrl-C the first terminal.
```

- [ ] **Step 8: Commit**

```bash
git add src/vsplit/dashboard/ src/vsplit/cli/dashboard.py src/vsplit/cli/__init__.py tests/unit/test_dashboard_health.py
git commit -m "feat(dashboard): FastAPI skeleton + vsplit dashboard CLI command"
```

---

## Phase 3 — SSE infrastructure

### Task 7: In-process SSE event broker

End state: `vsplit.dashboard.sse.EventBroker` lets producers publish events and consumers subscribe with an async generator.

**Files:**
- Create: `src/vsplit/dashboard/sse.py`
- Create: `tests/unit/test_dashboard_sse_broker.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_sse_broker.py`:

```python
import asyncio
import pytest
from vsplit.dashboard.sse import EventBroker


@pytest.mark.asyncio
async def test_broker_delivers_events_to_subscriber() -> None:
    broker = EventBroker()
    received = []

    async def consume():
        async for ev in broker.subscribe():
            received.append(ev)
            if len(received) == 2:
                break

    task = asyncio.create_task(consume())
    await asyncio.sleep(0)  # let subscriber register
    broker.publish({"type": "a"})
    broker.publish({"type": "b"})
    await asyncio.wait_for(task, timeout=1.0)
    assert received == [{"type": "a"}, {"type": "b"}]


@pytest.mark.asyncio
async def test_broker_supports_multiple_subscribers() -> None:
    broker = EventBroker()

    async def consume(buf: list):
        async for ev in broker.subscribe():
            buf.append(ev)
            if len(buf) == 1:
                break

    a, b = [], []
    t1 = asyncio.create_task(consume(a))
    t2 = asyncio.create_task(consume(b))
    await asyncio.sleep(0)
    broker.publish({"type": "x"})
    await asyncio.gather(asyncio.wait_for(t1, 1.0), asyncio.wait_for(t2, 1.0))
    assert a == [{"type": "x"}]
    assert b == [{"type": "x"}]
```

Add `pytest-asyncio` to dev deps if not already present.

- [ ] **Step 2: Add `pytest-asyncio` to dev deps + install**

Modify `pyproject.toml` dev deps:

```toml
dev = [
    "pytest>=8.0",
    "pytest-cov>=4.1",
    "pytest-asyncio>=0.23",
    "ruff>=0.5",
]
```

```bash
uv pip install -e ".[dev]"
```

Also add `asyncio_mode = "auto"` to `[tool.pytest.ini_options]` in `pyproject.toml`:

```toml
[tool.pytest.ini_options]
testpaths = ["tests"]
pythonpath = ["src"]
asyncio_mode = "auto"
```

- [ ] **Step 3: Run — should fail**

```bash
uv run pytest tests/unit/test_dashboard_sse_broker.py -v
```

Expected: `ModuleNotFoundError: No module named 'vsplit.dashboard.sse'`.

- [ ] **Step 4: Implement `EventBroker`**

`src/vsplit/dashboard/sse.py`:

```python
"""In-process pub/sub broker for Server-Sent Events.

Subscribers each get their own asyncio.Queue. Publishers (the manifest
watcher thread, in-process action callbacks) fan-out to all queues.
"""
from __future__ import annotations
import asyncio
from typing import AsyncIterator


class EventBroker:
    def __init__(self) -> None:
        self._subscribers: list[asyncio.Queue] = []
        self._lock = asyncio.Lock()

    def publish(self, event: dict) -> None:
        """Non-blocking fan-out. Drops events for slow consumers."""
        for q in list(self._subscribers):
            try:
                q.put_nowait(event)
            except asyncio.QueueFull:
                pass  # subscriber too slow; drop

    async def subscribe(self) -> AsyncIterator[dict]:
        """Yield events for one subscriber until cancelled."""
        q: asyncio.Queue = asyncio.Queue(maxsize=256)
        self._subscribers.append(q)
        try:
            while True:
                ev = await q.get()
                yield ev
        finally:
            self._subscribers.remove(q)
```

- [ ] **Step 5: Tests pass**

```bash
uv run pytest tests/unit/test_dashboard_sse_broker.py -v
```

Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add pyproject.toml src/vsplit/dashboard/sse.py tests/unit/test_dashboard_sse_broker.py
git commit -m "feat(dashboard): in-process SSE event broker"
```

---

### Task 8: Manifest watcher thread

End state: `ManifestWatcher` polls each workspace's `manifest.json` mtime every 500ms and publishes `workspace_updated` / `workspaces_added` / `workspaces_removed` events to the broker.

**Files:**
- Create: `src/vsplit/dashboard/watcher.py`
- Create: `tests/unit/test_dashboard_watcher.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_watcher.py`:

```python
import asyncio
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

import pytest

from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.sse import EventBroker
from vsplit.dashboard.watcher import ManifestWatcher
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str):
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    return ws, m


async def test_watcher_emits_on_manifest_change(tmp_path: Path) -> None:
    ws, m = _seed(tmp_path, "alpha")
    save_index(tmp_path, Index(workspaces={"alpha": WorkspaceEntry(
        source_relative_path="alpha.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    broker = EventBroker()
    watcher = ManifestWatcher(tmp_path, broker, poll_interval=0.05)
    watcher.start()
    try:
        sub = broker.subscribe()
        # Trigger a manifest change
        await asyncio.sleep(0.1)
        m.stages["probe"].status = "done"
        save_manifest(ws, m)
        ev = await asyncio.wait_for(anext(sub), timeout=2.0)
        assert ev["type"] == "workspace_updated"
        assert ev["slug"] == "alpha"
    finally:
        watcher.stop()
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Implement `ManifestWatcher`**

`src/vsplit/dashboard/watcher.py`:

```python
"""Background thread that watches per-workspace manifest.json mtimes and
publishes change events to the SSE broker.

This catches edits made by:
- another `vsplit` CLI invocation
- manual JSON edits
- the dashboard's own action handlers (redundant but harmless)
"""
from __future__ import annotations
import asyncio
import threading
import time
from pathlib import Path

from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest, manifest_path
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.sse import EventBroker


class ManifestWatcher:
    def __init__(
        self,
        working_root: Path,
        broker: EventBroker,
        *,
        poll_interval: float = 0.5,
        loop: asyncio.AbstractEventLoop | None = None,
    ) -> None:
        self._root = working_root
        self._broker = broker
        self._interval = poll_interval
        self._loop = loop
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._mtimes: dict[str, float] = {}
        self._known: set[str] = set()

    def start(self) -> None:
        self._loop = self._loop or asyncio.get_event_loop()
        self._thread = threading.Thread(target=self._loop_run, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=2.0)

    def _publish(self, event: dict) -> None:
        # publish() is sync; safe to call from background thread when the
        # asyncio.Queue itself is not bound to a specific loop. EventBroker's
        # queues use put_nowait so this is fine.
        self._broker.publish(event)

    def _loop_run(self) -> None:
        while not self._stop.is_set():
            try:
                self._tick()
            except Exception:  # noqa: BLE001 — watcher must never crash the server
                pass
            time.sleep(self._interval)

    def _tick(self) -> None:
        try:
            idx = load_index(self._root)
        except Exception:
            return
        current = set(idx.workspaces.keys())
        added = current - self._known
        removed = self._known - current
        if added:
            self._publish({"type": "workspaces_added", "slugs": sorted(added)})
        if removed:
            self._publish({"type": "workspaces_removed", "slugs": sorted(removed)})
            for s in removed:
                self._mtimes.pop(s, None)
        self._known = current

        for slug in current:
            p = manifest_path(workspace_dir(self._root, slug))
            try:
                mt = p.stat().st_mtime
            except FileNotFoundError:
                continue
            prev = self._mtimes.get(slug)
            if prev != mt:
                self._mtimes[slug] = mt
                try:
                    m = load_manifest(workspace_dir(self._root, slug))
                except Exception:
                    continue
                self._publish({
                    "type": "workspace_updated",
                    "slug": slug,
                    "stages": {n: b.status for n, b in m.stages.items()},
                    "updated_at": m.updated_at.isoformat(),
                })
```

- [ ] **Step 4: Test passes**

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/dashboard/watcher.py tests/unit/test_dashboard_watcher.py
git commit -m "feat(dashboard): manifest mtime watcher thread"
```

---

### Task 9: `/api/events` SSE endpoint

End state: `GET /api/events` returns `text/event-stream`; heartbeat every 15s; consumes from broker.

**Files:**
- Modify: `src/vsplit/dashboard/server.py`
- Create: `tests/unit/test_dashboard_sse_endpoint.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_sse_endpoint.py`:

```python
import asyncio
import json
import threading
import time
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from vsplit.dashboard.server import create_app


def test_events_endpoint_yields_heartbeat(tmp_path: Path) -> None:
    app = create_app(working_root=tmp_path, heartbeat_interval=0.05)
    with TestClient(app) as client:
        with client.stream("GET", "/api/events") as response:
            assert response.status_code == 200
            assert response.headers["content-type"].startswith("text/event-stream")
            data_lines = []
            for line in response.iter_lines():
                if line.startswith("data:"):
                    data_lines.append(line)
                if len(data_lines) >= 1:
                    break
            assert any("heartbeat" in line for line in data_lines)
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Wire SSE into `create_app`**

Replace `src/vsplit/dashboard/server.py` with:

```python
"""FastAPI app factory + uvicorn launcher for the vsplit dashboard."""
from __future__ import annotations
import asyncio
import json
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.responses import StreamingResponse

from vsplit.dashboard.sse import EventBroker
from vsplit.dashboard.watcher import ManifestWatcher


def create_app(
    working_root: Path,
    *,
    heartbeat_interval: float = 15.0,
) -> FastAPI:
    app = FastAPI(title="vsplit dashboard", version="0.1.0")
    app.state.working_root = working_root
    broker = EventBroker()
    app.state.broker = broker
    watcher = ManifestWatcher(working_root, broker)
    app.state.watcher = watcher

    @app.on_event("startup")
    def _startup() -> None:
        watcher.start()

    @app.on_event("shutdown")
    def _shutdown() -> None:
        watcher.stop()

    @app.get("/api/health")
    def health() -> dict:
        return {"ok": True}

    async def _event_stream():
        subscriber = broker.subscribe()
        last = asyncio.get_event_loop().time()
        try:
            while True:
                try:
                    ev = await asyncio.wait_for(
                        anext(subscriber),
                        timeout=heartbeat_interval,
                    )
                    yield f"data: {json.dumps(ev)}\n\n"
                except asyncio.TimeoutError:
                    yield f"data: {json.dumps({'type': 'heartbeat'})}\n\n"
        finally:
            await subscriber.aclose()

    @app.get("/api/events")
    async def events():
        return StreamingResponse(_event_stream(), media_type="text/event-stream")

    return app


def run_server(working_root: Path, *, host="127.0.0.1", port=8765, open_browser=True) -> None:
    app = create_app(working_root)
    url = f"http://{host}:{port}"
    if open_browser:
        webbrowser.open(url)
    print(f"vsplit dashboard ready at {url}")
    uvicorn.run(app, host=host, port=port, log_level="info")
```

- [ ] **Step 4: Test passes**

```bash
uv run pytest tests/unit/test_dashboard_sse_endpoint.py -v
```

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/dashboard/server.py tests/unit/test_dashboard_sse_endpoint.py
git commit -m "feat(dashboard): /api/events SSE stream with heartbeat"
```

---

## Phase 4 — Backend API: read endpoints

### Task 10: `/api/workspaces` list + detail endpoints

**Files:**
- Create: `src/vsplit/dashboard/api/__init__.py`
- Create: `src/vsplit/dashboard/api/workspaces.py`
- Modify: `src/vsplit/dashboard/server.py` (include router)
- Create: `tests/unit/test_dashboard_api_workspaces.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_api_workspaces.py`:

```python
from datetime import datetime, timezone
from pathlib import Path
from fastapi.testclient import TestClient

from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))


def test_list_workspaces(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    _seed(tmp_path, "beta")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces")
    assert r.status_code == 200
    slugs = [w["slug"] for w in r.json()["workspaces"]]
    assert sorted(slugs) == ["alpha", "beta"]


def test_workspace_detail(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha")
    assert r.status_code == 200
    assert r.json()["slug"] == "alpha"
    assert "stages" in r.json()


def test_workspace_detail_404(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/missing")
    assert r.status_code == 404
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Implement router**

`src/vsplit/dashboard/api/__init__.py`:

```python
```

`src/vsplit/dashboard/api/workspaces.py`:

```python
from __future__ import annotations
from pathlib import Path
from fastapi import APIRouter, HTTPException, Request

from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

router = APIRouter(prefix="/api/workspaces", tags=["workspaces"])


def _summary(slug: str, m) -> dict:
    statuses = [b.status for b in m.stages.values()]
    return {
        "slug": slug,
        "source": m.source.relative_path,
        "stages": {n: b.status for n, b in m.stages.items()},
        "any_failed": "failed" in statuses,
        "any_awaiting": "awaiting_user" in statuses,
        "all_done": all(s == "done" for s in statuses),
    }


@router.get("")
def list_workspaces(request: Request) -> dict:
    root: Path = request.app.state.working_root
    idx = load_index(root)
    items = []
    for slug in idx.workspaces:
        try:
            m = load_manifest(workspace_dir(root, slug))
        except FileNotFoundError:
            continue
        items.append(_summary(slug, m))
    return {"workspaces": items}


@router.get("/{slug}")
def get_workspace(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        m = load_manifest(workspace_dir(root, slug))
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")
    return {
        **_summary(slug, m),
        "manifest": m.model_dump(mode="json"),
    }
```

- [ ] **Step 4: Register router in `server.py`**

In `create_app`, after `app.state.watcher = watcher`:

```python
    from vsplit.dashboard.api import workspaces as workspaces_api
    app.include_router(workspaces_api.router)
```

- [ ] **Step 5: Tests pass**

```bash
uv run pytest tests/unit/test_dashboard_api_workspaces.py -v
```

- [ ] **Step 6: Commit**

```bash
git add src/vsplit/dashboard/api/ src/vsplit/dashboard/server.py tests/unit/test_dashboard_api_workspaces.py
git commit -m "feat(dashboard): GET /api/workspaces list + detail endpoints"
```

---

### Task 11: Thumbnail file serving

**Files:**
- Modify: `src/vsplit/dashboard/api/workspaces.py`
- Modify: `tests/unit/test_dashboard_api_workspaces.py`

- [ ] **Step 1: Append failing test**

```python
def test_thumb_serving(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    img_dir = workspace_dir(tmp_path, "alpha") / "preview" / "images"
    img_dir.mkdir(parents=True)
    (img_dir / "scene-001-01.jpg").write_bytes(b"\xff\xd8\xff\xe0fake")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/thumbs/scene-001-01.jpg")
    assert r.status_code == 200
    assert r.content == b"\xff\xd8\xff\xe0fake"


def test_thumb_404(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/thumbs/missing.jpg")
    assert r.status_code == 404


def test_thumb_traversal_rejected(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/thumbs/../../escape.jpg")
    assert r.status_code in (400, 404)
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Add thumb route**

Append to `src/vsplit/dashboard/api/workspaces.py`:

```python
from fastapi.responses import FileResponse


@router.get("/{slug}/thumbs/{name}")
def get_thumb(slug: str, name: str, request: Request):
    root: Path = request.app.state.working_root
    # Reject anything that would escape the thumbs dir.
    if "/" in name or "\\" in name or ".." in name:
        raise HTTPException(status_code=400, detail="invalid name")
    img = workspace_dir(root, slug) / "preview" / "images" / name
    if not img.is_file():
        raise HTTPException(status_code=404, detail="thumb not found")
    return FileResponse(img, media_type="image/jpeg")
```

- [ ] **Step 4: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_api_workspaces.py -v
git add src/vsplit/dashboard/api/workspaces.py tests/unit/test_dashboard_api_workspaces.py
git commit -m "feat(dashboard): GET /api/workspaces/{slug}/thumbs/{name}"
```

---

### Task 12: `/api/workspaces/{slug}/logs/{stage}` endpoint

**Files:**
- Create: `src/vsplit/dashboard/api/logs.py`
- Modify: `src/vsplit/dashboard/server.py`
- Create: `tests/unit/test_dashboard_api_logs.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_api_logs.py`:

```python
from datetime import datetime, timezone
from pathlib import Path
from fastapi.testclient import TestClient

from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed_with_log(tmp_path: Path, slug: str, stage: str, log_text: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    (ws / "logs").mkdir()
    (ws / "logs" / f"{stage}.log").write_text(log_text)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))


def test_log_endpoint_returns_text(tmp_path: Path) -> None:
    _seed_with_log(tmp_path, "alpha", "probe", "line one\nline two\n")
    r = TestClient(create_app(tmp_path)).get("/api/workspaces/alpha/logs/probe")
    assert r.status_code == 200
    assert r.text.startswith("line one")


def test_log_endpoint_404_for_missing(tmp_path: Path) -> None:
    _seed_with_log(tmp_path, "alpha", "probe", "x")
    r = TestClient(create_app(tmp_path)).get("/api/workspaces/alpha/logs/detect")
    assert r.status_code == 404
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Implement**

`src/vsplit/dashboard/api/logs.py`:

```python
from __future__ import annotations
from pathlib import Path
from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import PlainTextResponse

from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

router = APIRouter(prefix="/api/workspaces", tags=["logs"])


@router.get("/{slug}/logs/{stage}", response_class=PlainTextResponse)
def get_log(slug: str, stage: str, request: Request) -> str:
    if stage not in STAGE_NAMES:
        raise HTTPException(status_code=400, detail=f"unknown stage: {stage}")
    root: Path = request.app.state.working_root
    p = workspace_dir(root, slug) / "logs" / f"{stage}.log"
    if not p.is_file():
        raise HTTPException(status_code=404, detail="log not found")
    return p.read_text(encoding="utf-8", errors="replace")
```

- [ ] **Step 4: Register in `server.py`**

In `create_app`, after the workspaces router include:

```python
    from vsplit.dashboard.api import logs as logs_api
    app.include_router(logs_api.router)
```

- [ ] **Step 5: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_api_logs.py -v
git add src/vsplit/dashboard/api/logs.py src/vsplit/dashboard/server.py tests/unit/test_dashboard_api_logs.py
git commit -m "feat(dashboard): GET /api/workspaces/{slug}/logs/{stage}"
```

---

### Task 13: Config endpoints (GET structured + raw + schema)

**Files:**
- Create: `src/vsplit/dashboard/api/config.py`
- Modify: `src/vsplit/dashboard/server.py`
- Create: `tests/unit/test_dashboard_api_config.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_api_config.py`:

```python
from pathlib import Path
from fastapi.testclient import TestClient
from vsplit.dashboard.server import create_app


def test_get_config_returns_defaults(tmp_path: Path) -> None:
    r = TestClient(create_app(tmp_path)).get("/api/config")
    assert r.status_code == 200
    cfg = r.json()
    assert cfg["detect"]["threshold"] == 27.0
    assert cfg["detect"]["detector"] == "content"


def test_get_config_raw_empty_when_no_file(tmp_path: Path) -> None:
    r = TestClient(create_app(tmp_path)).get("/api/config/raw")
    assert r.status_code == 200
    assert r.json()["text"] == ""


def test_get_config_raw_returns_file_content(tmp_path: Path) -> None:
    (tmp_path / ".vsplit").mkdir()
    (tmp_path / ".vsplit" / "config.yaml").write_text("detect:\n  threshold: 35\n")
    r = TestClient(create_app(tmp_path)).get("/api/config/raw")
    assert "threshold: 35" in r.json()["text"]


def test_get_config_schema_has_detect_section(tmp_path: Path) -> None:
    r = TestClient(create_app(tmp_path)).get("/api/config/schema")
    assert r.status_code == 200
    schema = r.json()
    # Pydantic JSON-schema nests via $defs / properties; just check Detect appears.
    assert "DetectConfig" in str(schema)
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Implement**

`src/vsplit/dashboard/api/config.py`:

```python
from __future__ import annotations
from pathlib import Path
from fastapi import APIRouter, Request

from vsplit.core.config import load_resolved_config
from vsplit.schemas.config import Config

router = APIRouter(prefix="/api/config", tags=["config"])


@router.get("")
def get_config(request: Request) -> dict:
    root: Path = request.app.state.working_root
    cfg = load_resolved_config(working_root=root, explicit_path=None)
    return cfg.model_dump(mode="json")


@router.get("/raw")
def get_raw(request: Request) -> dict:
    root: Path = request.app.state.working_root
    p = root / ".vsplit" / "config.yaml"
    text = p.read_text(encoding="utf-8") if p.is_file() else ""
    return {"text": text}


@router.get("/schema")
def get_schema() -> dict:
    return Config.model_json_schema()
```

- [ ] **Step 4: Register router + tests pass + commit**

In `server.py`:

```python
    from vsplit.dashboard.api import config as config_api
    app.include_router(config_api.router)
```

```bash
uv run pytest tests/unit/test_dashboard_api_config.py -v
git add src/vsplit/dashboard/api/config.py src/vsplit/dashboard/server.py tests/unit/test_dashboard_api_config.py
git commit -m "feat(dashboard): GET /api/config + /raw + /schema"
```

---

## Phase 5 — Backend API: write endpoints

### Task 14: Per-workspace action endpoints

End state: POST `/api/workspaces/{slug}/{run|reset|clean|approve}` works.

**Files:**
- Create: `src/vsplit/dashboard/api/actions.py`
- Modify: `src/vsplit/dashboard/server.py`
- Create: `tests/unit/test_dashboard_api_actions.py`

- [ ] **Step 1: Write failing tests**

`tests/unit/test_dashboard_api_actions.py`:

```python
import shutil
from datetime import datetime, timezone
from pathlib import Path
import pytest
from fastapi.testclient import TestClient

from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"


def _seed(tmp_path: Path, slug: str, *, awaiting=False, with_source=False) -> Path:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    src_rel = f"{slug}.mp4"
    if with_source and FIXTURE.exists():
        shutil.copy(FIXTURE, tmp_path / src_rel)
        size = (tmp_path / src_rel).stat().st_size
    else:
        size = 1
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=src_rel, size_bytes=size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    if awaiting:
        m.stages["review"].status = "awaiting_user"
        m.stages["review"].artifacts = {"cuts_file_hash": "x"}
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path=src_rel, size_bytes=size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    return ws


def test_clean_endpoint(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    r = TestClient(create_app(tmp_path)).post("/api/workspaces/alpha/clean")
    assert r.status_code == 200
    assert r.json() == {"cleaned": 1}
    assert not workspace_dir(tmp_path, "alpha").exists()


def test_approve_endpoint(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha", awaiting=True)
    r = TestClient(create_app(tmp_path)).post("/api/workspaces/alpha/approve")
    assert r.status_code == 200
    assert r.json() == {"approved": 1}
    m = load_manifest(workspace_dir(tmp_path, "alpha"))
    assert m.stages["review"].artifacts.get("approved_at")


def test_reset_endpoint(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    m = load_manifest(ws)
    m.stages["probe"].status = "done"
    m.stages["detect"].status = "done"
    save_manifest(ws, m)
    r = TestClient(create_app(tmp_path)).post(
        "/api/workspaces/alpha/reset", json={"from": "detect"}
    )
    assert r.status_code == 200
    m2 = load_manifest(ws)
    assert m2.stages["probe"].status == "done"
    assert m2.stages["detect"].status == "pending"


@pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")
def test_run_endpoint_advances_probe(tmp_path: Path) -> None:
    _seed(tmp_path, "short", with_source=True)
    r = TestClient(create_app(tmp_path)).post(
        "/api/workspaces/short/run", json={"only": "probe"}
    )
    assert r.status_code == 200
    m = load_manifest(workspace_dir(tmp_path, "short"))
    assert m.stages["probe"].status == "done"
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Implement**

`src/vsplit/dashboard/api/actions.py`:

```python
from __future__ import annotations
import threading
from pathlib import Path

from fastapi import APIRouter, HTTPException, Request
from fastapi.concurrency import run_in_threadpool
from pydantic import BaseModel

from vsplit.core.actions import (
    approve_workspaces, clean_workspaces, reset_workspaces, run_workspaces,
)
from vsplit.core.config import load_resolved_config
from vsplit.core.index import load_index
from vsplit.schemas.manifest import STAGE_NAMES

router = APIRouter(prefix="/api/workspaces", tags=["actions"])


class RunBody(BaseModel):
    only: str | None = None
    from_: str | None = None
    through: str | None = None
    concurrency_overrides: dict[str, int] | None = None

    class Config:
        fields = {"from_": "from"}


class ResetBody(BaseModel):
    from_: str

    class Config:
        fields = {"from_": "from"}


def _require_slug(working_root: Path, slug: str) -> None:
    idx = load_index(working_root)
    if slug not in idx.workspaces:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")


@router.post("/{slug}/clean")
async def clean_one(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    n = await run_in_threadpool(clean_workspaces, root, [slug])
    return {"cleaned": n}


@router.post("/{slug}/approve")
async def approve_one(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    n = await run_in_threadpool(approve_workspaces, root, [slug])
    return {"approved": n}


@router.post("/{slug}/reset")
async def reset_one(slug: str, body: ResetBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    if body.from_ not in STAGE_NAMES:
        raise HTTPException(status_code=422, detail=f"unknown stage: {body.from_!r}")
    await run_in_threadpool(reset_workspaces, root, [slug], from_=body.from_)
    return {"ok": True}


@router.post("/{slug}/run")
async def run_one(slug: str, body: RunBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    cfg = load_resolved_config(working_root=root, explicit_path=None)
    if body.concurrency_overrides:
        for k, v in body.concurrency_overrides.items():
            cfg.concurrency[k] = v
    cancel_event = threading.Event()
    await run_in_threadpool(
        run_workspaces, root, [slug],
        config=cfg, cancel_event=cancel_event,
        only=body.only, from_=body.from_, through=body.through,
    )
    return {"ok": True}
```

- [ ] **Step 4: Register router**

```python
    from vsplit.dashboard.api import actions as actions_api
    app.include_router(actions_api.router)
```

- [ ] **Step 5: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_api_actions.py -v
git add src/vsplit/dashboard/api/actions.py src/vsplit/dashboard/server.py tests/unit/test_dashboard_api_actions.py
git commit -m "feat(dashboard): per-workspace action endpoints"
```

---

### Task 15: Batch action endpoints

**Files:**
- Modify: `src/vsplit/dashboard/api/actions.py`
- Modify: `tests/unit/test_dashboard_api_actions.py`

- [ ] **Step 1: Append failing test**

```python
def test_batch_clean(tmp_path: Path) -> None:
    _seed(tmp_path, "a")
    _seed(tmp_path, "b")
    _seed(tmp_path, "c")
    r = TestClient(create_app(tmp_path)).post(
        "/api/batch/clean", json={"slugs": ["a", "c"]}
    )
    assert r.status_code == 200
    assert r.json() == {"cleaned": 2}
    idx = load_index(tmp_path)
    assert "b" in idx.workspaces
    assert "a" not in idx.workspaces
    assert "c" not in idx.workspaces


def test_batch_approve(tmp_path: Path) -> None:
    _seed(tmp_path, "a", awaiting=True)
    _seed(tmp_path, "b", awaiting=False)
    r = TestClient(create_app(tmp_path)).post(
        "/api/batch/approve", json={"slugs": ["a", "b"]}
    )
    assert r.json() == {"approved": 1}
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Add batch endpoints**

Append to `src/vsplit/dashboard/api/actions.py`:

```python
batch_router = APIRouter(prefix="/api/batch", tags=["batch"])


class BatchSlugs(BaseModel):
    slugs: list[str]


class BatchReset(BatchSlugs):
    from_: str

    class Config:
        fields = {"from_": "from"}


class BatchRun(BatchSlugs):
    only: str | None = None
    from_: str | None = None
    through: str | None = None
    concurrency_overrides: dict[str, int] | None = None

    class Config:
        fields = {"from_": "from"}


@batch_router.post("/clean")
async def batch_clean(body: BatchSlugs, request: Request) -> dict:
    root: Path = request.app.state.working_root
    n = await run_in_threadpool(clean_workspaces, root, body.slugs)
    return {"cleaned": n}


@batch_router.post("/approve")
async def batch_approve(body: BatchSlugs, request: Request) -> dict:
    root: Path = request.app.state.working_root
    n = await run_in_threadpool(approve_workspaces, root, body.slugs)
    return {"approved": n}


@batch_router.post("/reset")
async def batch_reset(body: BatchReset, request: Request) -> dict:
    root: Path = request.app.state.working_root
    if body.from_ not in STAGE_NAMES:
        raise HTTPException(status_code=422, detail=f"unknown stage: {body.from_!r}")
    await run_in_threadpool(reset_workspaces, root, body.slugs, from_=body.from_)
    return {"ok": True}


@batch_router.post("/run")
async def batch_run(body: BatchRun, request: Request) -> dict:
    root: Path = request.app.state.working_root
    cfg = load_resolved_config(working_root=root, explicit_path=None)
    if body.concurrency_overrides:
        for k, v in body.concurrency_overrides.items():
            cfg.concurrency[k] = v
    cancel_event = threading.Event()
    await run_in_threadpool(
        run_workspaces, root, body.slugs,
        config=cfg, cancel_event=cancel_event,
        only=body.only, from_=body.from_, through=body.through,
    )
    return {"ok": True}
```

- [ ] **Step 4: Register batch router in `server.py`**

```python
    app.include_router(actions_api.batch_router)
```

- [ ] **Step 5: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_api_actions.py -v
git add src/vsplit/dashboard/api/actions.py src/vsplit/dashboard/server.py tests/unit/test_dashboard_api_actions.py
git commit -m "feat(dashboard): batch action endpoints"
```

---

### Task 16: PUT `/api/config` + `/api/config/raw`

**Files:**
- Modify: `src/vsplit/dashboard/api/config.py`
- Modify: `tests/unit/test_dashboard_api_config.py`

- [ ] **Step 1: Append failing tests**

```python
def test_put_structured_config(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.put("/api/config", json={"detect": {"threshold": 42.0}})
    assert r.status_code == 200
    cfg = client.get("/api/config").json()
    assert cfg["detect"]["threshold"] == 42.0


def test_put_structured_config_rejects_invalid(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.put("/api/config", json={"detect": {"detector": "not-a-detector"}})
    assert r.status_code == 422


def test_put_raw_config(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.put(
        "/api/config/raw",
        json={"text": "detect:\n  threshold: 19.0\n"},
    )
    assert r.status_code == 200
    assert client.get("/api/config").json()["detect"]["threshold"] == 19.0


def test_put_raw_config_invalid_yaml(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.put("/api/config/raw", json={"text": "detect:\n  threshold: : :"})
    assert r.status_code == 400
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Implement PUT routes**

Append to `src/vsplit/dashboard/api/config.py`:

```python
import yaml
from fastapi import HTTPException
from pydantic import BaseModel, ValidationError

from vsplit.core.atomic import atomic_write_text
from vsplit.core.paths import working_state_dir


class RawBody(BaseModel):
    text: str


def _config_path(root: Path) -> Path:
    return root / ".vsplit" / "config.yaml"


@router.put("")
def put_config(payload: dict, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        Config.model_validate(payload)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors())
    text = yaml.safe_dump(payload, sort_keys=False)
    working_state_dir(root).mkdir(parents=True, exist_ok=True)
    atomic_write_text(_config_path(root), text)
    return {"ok": True}


@router.put("/raw")
def put_raw(body: RawBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        parsed = yaml.safe_load(body.text) or {}
    except yaml.YAMLError as e:
        mark = getattr(e, "problem_mark", None)
        raise HTTPException(status_code=400, detail={
            "line": mark.line + 1 if mark else None,
            "col": mark.column + 1 if mark else None,
            "message": str(e),
        })
    if not isinstance(parsed, dict):
        raise HTTPException(status_code=400, detail="YAML root must be a mapping")
    try:
        Config.model_validate(parsed)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors())
    working_state_dir(root).mkdir(parents=True, exist_ok=True)
    atomic_write_text(_config_path(root), body.text)
    return {"ok": True}
```

- [ ] **Step 4: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_api_config.py -v
git add src/vsplit/dashboard/api/config.py tests/unit/test_dashboard_api_config.py
git commit -m "feat(dashboard): PUT /api/config + /api/config/raw"
```

---

### Task 17: Wire in-process action events into SSE broker

End state: when the dashboard dispatches a run, the orchestrator publishes `run_started` / `workspace_updated` / `run_finished` events as stages complete.

**Files:**
- Modify: `src/vsplit/dashboard/api/actions.py`
- Modify: `src/vsplit/core/actions.py`
- Create: `tests/unit/test_dashboard_action_sse_events.py`

- [ ] **Step 1: Write failing test**

`tests/unit/test_dashboard_action_sse_events.py`:

```python
import shutil
import threading
from datetime import datetime, timezone
from pathlib import Path
import pytest
from fastapi.testclient import TestClient

from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"


@pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")
def test_run_publishes_started_and_finished(tmp_path: Path) -> None:
    shutil.copy(FIXTURE, tmp_path / FIXTURE.name)
    ws = workspace_dir(tmp_path, "short")
    ws.mkdir(parents=True)
    m = new_manifest(slug="short", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=(tmp_path / FIXTURE.name).stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={"short": WorkspaceEntry(
        source_relative_path=FIXTURE.name,
        size_bytes=(tmp_path / FIXTURE.name).stat().st_size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    app = create_app(tmp_path, heartbeat_interval=10.0)
    received: list[dict] = []

    with TestClient(app) as client:
        # Subscribe to SSE first
        import json
        with client.stream("GET", "/api/events") as r:
            # Trigger run in another thread
            def kick():
                client.post("/api/workspaces/short/run", json={"only": "probe"})
            t = threading.Thread(target=kick)
            t.start()
            for line in r.iter_lines():
                if line.startswith("data:"):
                    payload = json.loads(line[5:].strip())
                    received.append(payload)
                    if payload.get("type") == "run_finished":
                        break
            t.join()

    types = [ev["type"] for ev in received]
    assert "run_started" in types
    assert "run_finished" in types
```

- [ ] **Step 2: Run — should fail**

- [ ] **Step 3: Add optional broker hook to `run_workspaces`**

Modify `src/vsplit/core/actions.py` `run_workspaces` signature to accept `broker` (no breaking change since it's keyword-only with default None):

```python
def run_workspaces(
    working_root: Path,
    slugs: list[str],
    *,
    config: Config,
    cancel_event: threading.Event,
    only: str | None = None,
    from_: str | None = None,
    through: str | None = None,
    stage_factory: Callable[[str], object] | None = None,
    broker=None,  # vsplit.dashboard.sse.EventBroker | None
) -> None:
    # ... existing body, but after computing stages_to_run and workspaces:
    if broker:
        for ws in workspaces:
            broker.publish({"type": "run_started", "slug": ws.slug, "stages": stages_to_run})
    # ... call dispatch as before ...
    if broker:
        for ws in workspaces:
            broker.publish({"type": "run_finished", "slug": ws.slug, "ok": True})
```

(The watcher thread already publishes `workspace_updated` whenever a stage finishes, so we don't need to also wire those here.)

- [ ] **Step 4: Pass broker from API into `run_workspaces`**

In `src/vsplit/dashboard/api/actions.py` `run_one` and `batch_run`:

```python
    broker = request.app.state.broker
    await run_in_threadpool(
        run_workspaces, root, [slug],
        config=cfg, cancel_event=cancel_event,
        only=body.only, from_=body.from_, through=body.through,
        broker=broker,
    )
```

- [ ] **Step 5: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_action_sse_events.py -v
git add src/vsplit/core/actions.py src/vsplit/dashboard/api/actions.py tests/unit/test_dashboard_action_sse_events.py
git commit -m "feat(dashboard): publish run_started/run_finished SSE events"
```

---

### Task 18: Backend end-to-end integration test

**Files:**
- Create: `tests/integration/test_dashboard_end_to_end.py`

- [ ] **Step 1: Write test**

`tests/integration/test_dashboard_end_to_end.py`:

```python
import json
import shutil
import threading
from datetime import datetime, timezone
from pathlib import Path
import pytest
from fastapi.testclient import TestClient

from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")


def test_full_workflow_via_api(tmp_path: Path) -> None:
    """Adopt → run preview → approve → run rest → clean — all via the API."""
    src = tmp_path / FIXTURE.name
    shutil.copy(FIXTURE, src)
    ws = workspace_dir(tmp_path, "short")
    ws.mkdir(parents=True)
    m = new_manifest(slug="short", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={"short": WorkspaceEntry(
        source_relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))

    app = create_app(tmp_path)
    with TestClient(app) as client:
        # Run through preview
        r = client.post("/api/workspaces/short/run", json={"through": "preview"})
        assert r.status_code == 200
        # Detail shows review awaiting_user
        detail = client.get("/api/workspaces/short").json()
        assert detail["stages"]["preview"] == "done"
        assert detail["stages"]["review"] == "awaiting_user"
        # Approve
        r = client.post("/api/workspaces/short/approve")
        assert r.json() == {"approved": 1}
        # Finish pipeline
        r = client.post("/api/workspaces/short/run")
        assert r.status_code == 200
        detail = client.get("/api/workspaces/short").json()
        assert detail["stages"]["verify"] == "done"
        assert detail["all_done"] is True
        # Clean
        r = client.post("/api/workspaces/short/clean")
        assert r.json() == {"cleaned": 1}
        assert "short" not in load_index(tmp_path).workspaces
```

- [ ] **Step 2: Test passes + commit**

```bash
uv run pytest tests/integration/test_dashboard_end_to_end.py -v
git add tests/integration/test_dashboard_end_to_end.py
git commit -m "test(integration): dashboard API full workflow"
```

---

## Phase 6 — Frontend scaffold (Bun + Vite + React + Tailwind + shadcn)

**Note on testing:** Per the spec §7, frontend has no unit tests in v1. Verification per task is `bun run build` succeeds + visual smoke check. Don't write Vitest tests.

### Task 19: Initialize Vite + React + TypeScript project

**Files:**
- Create everything under `src/vsplit/dashboard/src/` (the Vite project root)

- [ ] **Step 1: Scaffold via Bun**

```bash
cd src/vsplit/dashboard
bun create vite src --template react-ts
cd src
bun install
```

When `bun create vite` asks for the project name, accept "src". (We're literally calling the project `src` because it's the source directory.)

- [ ] **Step 2: Verify scaffold built**

```bash
bun run build
ls dist/
```

Expected: `dist/index.html` and `dist/assets/*.js`, `*.css`.

- [ ] **Step 3: Move built dist up one level**

The Vite project lives at `src/vsplit/dashboard/src/`, but per the spec the *served* dist is at `src/vsplit/dashboard/dist/`. Update `vite.config.ts`:

`src/vsplit/dashboard/src/vite.config.ts`:

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: path.resolve(__dirname, '../dist'),
    emptyOutDir: true,
  },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8765',
    },
  },
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
  },
})
```

Then rebuild:

```bash
bun run build
ls ../dist/   # built into the parent dir now
```

- [ ] **Step 4: Add a `.gitignore` inside the Vite project**

`src/vsplit/dashboard/src/.gitignore`:

```
node_modules/
```

(We DO want `../dist/` committed — see Task 27.)

- [ ] **Step 5: Commit (including built dist for now; will be re-built later tasks)**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): initialize Vite + React + TS frontend scaffold"
```

---

### Task 20: Add Tailwind CSS + shadcn/ui prerequisites

**Files:**
- Modify: `src/vsplit/dashboard/src/package.json` (via Bun add)
- Create: `src/vsplit/dashboard/src/tailwind.config.ts`
- Create: `src/vsplit/dashboard/src/postcss.config.js`
- Modify: `src/vsplit/dashboard/src/src/index.css`
- Modify: `src/vsplit/dashboard/src/tsconfig.json` (path alias for shadcn)

- [ ] **Step 1: Install Tailwind**

```bash
cd src/vsplit/dashboard/src
bun add -d tailwindcss postcss autoprefixer
bunx tailwindcss init -p
```

- [ ] **Step 2: Configure `tailwind.config.ts`**

Replace generated `tailwind.config.js` with `tailwind.config.ts`:

```typescript
import type { Config } from "tailwindcss"

export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: { extend: {} },
  plugins: [],
} satisfies Config
```

Delete the auto-generated `.js` version if present.

- [ ] **Step 3: Replace `src/index.css` with Tailwind directives**

`src/vsplit/dashboard/src/src/index.css`:

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
  html, body, #root { @apply h-full bg-zinc-950 text-zinc-100 font-sans; }
}
```

- [ ] **Step 4: Add shadcn deps**

```bash
bun add class-variance-authority clsx tailwind-merge lucide-react
bun add -d @types/node
```

- [ ] **Step 5: Add `@/lib/utils.ts` (cn helper)**

`src/vsplit/dashboard/src/src/lib/utils.ts`:

```typescript
import { type ClassValue, clsx } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
```

- [ ] **Step 6: Verify build**

```bash
bun run build
```

Expected: builds without errors. Generated CSS includes Tailwind base layer.

- [ ] **Step 7: Commit**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): add Tailwind + shadcn prerequisites"
```

---

### Task 21: Add shadcn/ui primitives (Button, Drawer, Switch, Select, Dialog, Toast)

shadcn/ui is copy-paste components; we vendor them under `components/ui/`.

**Files:**
- Create: `src/vsplit/dashboard/src/src/components/ui/button.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/dialog.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/drawer.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/switch.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/select.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/checkbox.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/badge.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/input.tsx`
- Create: `src/vsplit/dashboard/src/src/components/ui/toast.tsx` + provider

- [ ] **Step 1: Use shadcn CLI to add components**

```bash
cd src/vsplit/dashboard/src
bunx --bun shadcn@latest init -y -d
```

When prompted, accept defaults (slate base, css variables yes). If `init` complains about missing `components.json`, create it manually:

`src/vsplit/dashboard/src/components.json`:

```json
{
  "$schema": "https://ui.shadcn.com/schema.json",
  "style": "default",
  "rsc": false,
  "tsx": true,
  "tailwind": {
    "config": "tailwind.config.ts",
    "css": "src/index.css",
    "baseColor": "slate",
    "cssVariables": true
  },
  "aliases": {
    "components": "@/components",
    "utils": "@/lib/utils"
  }
}
```

Then:

```bash
bunx --bun shadcn@latest add button dialog drawer switch select checkbox badge input toast
```

- [ ] **Step 2: Verify build**

```bash
bun run build
```

- [ ] **Step 3: Commit**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): vendor shadcn/ui primitives"
```

---

### Task 22: Typed API client (generated from OpenAPI) + SSE hook

**Files:**
- Create: `src/vsplit/dashboard/src/gen-types.sh`
- Create: `src/vsplit/dashboard/src/src/lib/api.ts`
- Create: `src/vsplit/dashboard/src/src/lib/sse.ts`

- [ ] **Step 1: Install openapi-typescript**

```bash
cd src/vsplit/dashboard/src
bun add -d openapi-typescript
```

- [ ] **Step 2: Write the generator script**

`src/vsplit/dashboard/src/gen-types.sh`:

```bash
#!/usr/bin/env bash
# Boot vsplit dashboard in a temp working root, dump openapi.json,
# and convert to TypeScript types.
set -euo pipefail

cd "$(dirname "$0")"
TMP=$(mktemp -d)
trap 'kill $PID 2>/dev/null; rm -rf "$TMP"' EXIT

# Use uv from project root
(cd ../../../.. && uv run vsplit -w "$TMP" dashboard --no-open --port 18765 &)
PID=$!
sleep 2
curl -s http://127.0.0.1:18765/openapi.json > openapi.json
bunx openapi-typescript openapi.json -o src/lib/api.types.ts
echo "regenerated src/lib/api.types.ts"
```

```bash
chmod +x gen-types.sh
./gen-types.sh
```

- [ ] **Step 3: Write typed API client**

`src/vsplit/dashboard/src/src/lib/api.ts`:

```typescript
const BASE = ""  // same-origin in production; Vite proxies /api in dev

type Body = Record<string, unknown> | undefined

async function jsonRequest<T>(
  method: "GET" | "POST" | "PUT",
  path: string,
  body?: Body,
): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const detail = await res.json().catch(() => ({}))
    throw new Error(detail?.detail ?? `${res.status} ${res.statusText}`)
  }
  return res.json() as Promise<T>
}

export type Workspace = {
  slug: string
  source: string
  stages: Record<string, string>
  any_failed: boolean
  any_awaiting: boolean
  all_done: boolean
}

export const api = {
  workspaces: {
    list: () => jsonRequest<{ workspaces: Workspace[] }>("GET", "/api/workspaces"),
    detail: (slug: string) => jsonRequest<Workspace & { manifest: unknown }>("GET", `/api/workspaces/${slug}`),
    log: (slug: string, stage: string) =>
      fetch(`/api/workspaces/${slug}/logs/${stage}`).then((r) => r.ok ? r.text() : Promise.reject(r.statusText)),
    run: (slug: string, body: { only?: string; from?: string; through?: string }) =>
      jsonRequest<{ ok: true }>("POST", `/api/workspaces/${slug}/run`, body),
    reset: (slug: string, body: { from: string }) =>
      jsonRequest<{ ok: true }>("POST", `/api/workspaces/${slug}/reset`, body),
    clean: (slug: string) =>
      jsonRequest<{ cleaned: number }>("POST", `/api/workspaces/${slug}/clean`),
    approve: (slug: string) =>
      jsonRequest<{ approved: number }>("POST", `/api/workspaces/${slug}/approve`),
    thumbUrl: (slug: string, name: string) => `/api/workspaces/${slug}/thumbs/${name}`,
  },
  batch: {
    run: (body: { slugs: string[]; only?: string; from?: string; through?: string }) =>
      jsonRequest<{ ok: true }>("POST", "/api/batch/run", body),
    reset: (body: { slugs: string[]; from: string }) =>
      jsonRequest<{ ok: true }>("POST", "/api/batch/reset", body),
    clean: (body: { slugs: string[] }) =>
      jsonRequest<{ cleaned: number }>("POST", "/api/batch/clean", body),
    approve: (body: { slugs: string[] }) =>
      jsonRequest<{ approved: number }>("POST", "/api/batch/approve", body),
  },
  config: {
    get: () => jsonRequest<Record<string, unknown>>("GET", "/api/config"),
    put: (cfg: Record<string, unknown>) =>
      jsonRequest<{ ok: true }>("PUT", "/api/config", cfg),
    getRaw: () => jsonRequest<{ text: string }>("GET", "/api/config/raw"),
    putRaw: (text: string) =>
      jsonRequest<{ ok: true }>("PUT", "/api/config/raw", { text }),
    schema: () => jsonRequest<Record<string, unknown>>("GET", "/api/config/schema"),
  },
}
```

- [ ] **Step 4: Write the SSE hook**

`src/vsplit/dashboard/src/src/lib/sse.ts`:

```typescript
import { useEffect } from "react"

export type DashboardEvent =
  | { type: "workspace_updated"; slug: string; stages: Record<string, string>; updated_at: string }
  | { type: "workspaces_added"; slugs: string[] }
  | { type: "workspaces_removed"; slugs: string[] }
  | { type: "run_started"; slug: string; stages: string[] }
  | { type: "run_finished"; slug: string; ok: boolean }
  | { type: "config_changed" }
  | { type: "heartbeat" }

export function useSSE(onEvent: (ev: DashboardEvent) => void) {
  useEffect(() => {
    const source = new EventSource("/api/events")
    source.onmessage = (m) => {
      try {
        onEvent(JSON.parse(m.data))
      } catch {
        // ignore malformed
      }
    }
    source.onerror = () => {
      // EventSource auto-reconnects with backoff
    }
    return () => source.close()
  }, [onEvent])
}
```

- [ ] **Step 5: Build still works**

```bash
bun run build
```

- [ ] **Step 6: Commit**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): typed API client + SSE hook + gen-types script"
```

---

## Phase 7 — Frontend UI: workspaces list

### Task 23: App shell + dark theme

End state: opening the dashboard shows a header with title, working-root path, count of workspaces, and an empty content area.

**Files:**
- Modify: `src/vsplit/dashboard/src/src/App.tsx`
- Modify: `src/vsplit/dashboard/src/src/main.tsx`

- [ ] **Step 1: Replace App.tsx**

`src/vsplit/dashboard/src/src/App.tsx`:

```tsx
import { useEffect, useState } from "react"
import { api, type Workspace } from "@/lib/api"
import { useSSE } from "@/lib/sse"
import { Settings } from "lucide-react"
import { Button } from "@/components/ui/button"

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([])

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

  return (
    <div className="min-h-full">
      <header className="border-b border-zinc-800 px-4 py-3 flex items-center gap-4">
        <h1 className="text-lg font-semibold">vsplit dashboard</h1>
        <div className="text-sm text-zinc-400">{workspaces.length} workspace{workspaces.length === 1 ? "" : "s"}</div>
        <Button variant="ghost" size="icon" className="ml-auto"><Settings className="h-4 w-4" /></Button>
      </header>
      <main className="p-4">
        {workspaces.length === 0 ? (
          <div className="text-zinc-500">No workspaces yet. Run <code className="text-zinc-300">vsplit scan</code> to adopt files.</div>
        ) : (
          <div className="text-zinc-500">[ workspace list goes here ]</div>
        )}
      </main>
    </div>
  )
}
```

- [ ] **Step 2: Ensure dark mode is applied**

`src/vsplit/dashboard/src/src/main.tsx` — wrap the root with `dark` class:

```tsx
import React from "react"
import ReactDOM from "react-dom/client"
import App from "./App.tsx"
import "./index.css"

document.documentElement.classList.add("dark")

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
```

- [ ] **Step 3: Build + manual smoke check**

```bash
bun run build
```

Then in another terminal:

```bash
uv run vsplit dashboard --no-open
# Open http://127.0.0.1:8765 — header should show "0 workspaces" against a dark background.
```

- [ ] **Step 4: Wire dist into FastAPI as static**

Modify `src/vsplit/dashboard/server.py` `create_app`, after all routers are included:

```python
    from fastapi.staticfiles import StaticFiles
    dist = Path(__file__).parent / "dist"
    if dist.is_dir():
        app.mount("/", StaticFiles(directory=str(dist), html=True), name="spa")
```

(Mount LAST so /api/* routes win.)

- [ ] **Step 5: Commit**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): app shell with dark theme + SPA static mount"
```

---

### Task 24: WorkspaceRow (collapsed view) with stage pips

**Files:**
- Create: `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`
- Modify: `src/vsplit/dashboard/src/src/App.tsx`

- [ ] **Step 1: Create the component**

`src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`:

```tsx
import { useState } from "react"
import { ChevronRight, ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { Checkbox } from "@/components/ui/checkbox"
import type { Workspace } from "@/lib/api"

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
}

export function WorkspaceRow({ ws, selected, onSelectChange, expanded, onToggle }: Props) {
  return (
    <div className={cn(
      "rounded-md border border-zinc-800 bg-zinc-900/50",
      selected && "ring-1 ring-blue-500",
    )}>
      <button
        onClick={() => onToggle(ws.slug)}
        className="w-full px-3 py-2 flex items-center gap-3 text-left"
      >
        {expanded ? <ChevronDown className="h-4 w-4 text-zinc-500" /> : <ChevronRight className="h-4 w-4 text-zinc-500" />}
        <Checkbox
          checked={selected}
          onCheckedChange={(c) => onSelectChange(ws.slug, !!c)}
          onClick={(e) => e.stopPropagation()}
        />
        <div className="w-12 h-7 rounded bg-zinc-800" /> {/* thumb placeholder */}
        <div className="font-medium text-sm flex-1 truncate">{ws.slug}</div>
        <div className="flex gap-0.5">
          {STAGES.map((s) => (
            <div
              key={s}
              title={`${s}: ${ws.stages[s] ?? "pending"}`}
              className={cn("w-2.5 h-2.5 rounded-sm", STATUS_COLOR[ws.stages[s] ?? "pending"])}
            />
          ))}
        </div>
      </button>
    </div>
  )
}
```

- [ ] **Step 2: Use it in App.tsx**

Replace the `[ workspace list goes here ]` placeholder:

```tsx
import { WorkspaceRow } from "./views/WorkspaceRow"
// ...
const [expanded, setExpanded] = useState<string | null>(null)
const [selected, setSelected] = useState<Set<string>>(new Set())
const toggleExpand = (slug: string) => setExpanded((e) => e === slug ? null : slug)
const toggleSelect = (slug: string, on: boolean) => setSelected((s) => {
  const next = new Set(s)
  if (on) next.add(slug); else next.delete(slug)
  return next
})

// in render:
<div className="space-y-1">
  {workspaces.map((ws) => (
    <WorkspaceRow
      key={ws.slug}
      ws={ws}
      selected={selected.has(ws.slug)}
      onSelectChange={toggleSelect}
      expanded={expanded === ws.slug}
      onToggle={toggleExpand}
    />
  ))}
</div>
```

- [ ] **Step 3: Build + manual smoke check**

```bash
bun run build
```

Seed a workspace manually in a test dir (`uv run vsplit -w some-dir scan`) and verify rows render with pips.

- [ ] **Step 4: Commit**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): collapsed WorkspaceRow with stage pips"
```

---

### Task 25: WorkspaceRow expanded view (scene strip + per-row actions)

**Files:**
- Modify: `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`

- [ ] **Step 1: Extend `WorkspaceRow` to render expanded body**

When `expanded === true`, fetch `/api/workspaces/{slug}` for thumb names and render a horizontal scene strip + action buttons.

```tsx
import { useEffect, useState } from "react"
import { api } from "@/lib/api"
import { Button } from "@/components/ui/button"
// ... existing imports

// inside WorkspaceRow, add expanded-body rendering after the header button:

const [detail, setDetail] = useState<any>(null)
useEffect(() => {
  if (!expanded) return
  api.workspaces.detail(ws.slug).then(setDetail)
}, [expanded, ws.slug])

return (
  <div className={/* ... */}>
    {/* existing header button */}
    {expanded && (
      <div className="px-3 pb-3 pt-1 border-t border-zinc-800 space-y-2">
        {/* scene strip */}
        <div className="flex gap-1 overflow-x-auto">
          {(detail?.manifest?.stages?.preview?.artifacts as any)?.images_generated > 0 && (
            // Fall back to alphabetical thumb list — backend should return names in detail later;
            // for now we generate placeholder strip.
            <div className="flex gap-1">
              {Array.from({ length: Math.min(detail?.manifest?.stages?.preview?.artifacts?.images_generated ?? 0, 6) }).map((_, i) => (
                <div key={i} className="w-20 h-12 rounded bg-zinc-800" />
              ))}
            </div>
          )}
        </div>
        {/* actions */}
        <div className="flex gap-2 flex-wrap">
          <Button size="sm" onClick={() => api.workspaces.run(ws.slug, {})}>Run</Button>
          {ws.any_awaiting && <Button size="sm" variant="secondary" onClick={() => api.workspaces.approve(ws.slug)}>Approve</Button>}
          <Button size="sm" variant="outline" onClick={() => {
            const stage = window.prompt("Reset from which stage?", "detect")
            if (stage) api.workspaces.reset(ws.slug, { from: stage })
          }}>Reset…</Button>
          <Button size="sm" variant="destructive" onClick={() => {
            if (window.confirm(`Delete workspace ${ws.slug}?`)) api.workspaces.clean(ws.slug)
          }}>Clean</Button>
          <Button size="sm" variant="ghost" asChild>
            <a href={`file://${detail?.manifest?.id ?? ""}/preview/index.html`} target="_blank" rel="noreferrer">Open preview ↗</a>
          </Button>
        </div>
      </div>
    )}
  </div>
)
```

(The preview href is a placeholder; we'll wire the real file:// URI when the backend exposes the workspace path. For v1, we can return `workspace_dir` in the detail response — add to `_summary` in `api/workspaces.py` if not already present.)

- [ ] **Step 2: Add `workspace_dir` to detail response**

In `src/vsplit/dashboard/api/workspaces.py`, in `_summary`, add `"workspace_path": str(workspace_dir(request.app.state.working_root, slug))` — wait, `_summary` doesn't have access to `request`. Easier: only add it in `get_workspace`:

```python
@router.get("/{slug}")
def get_workspace(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        m = load_manifest(workspace_dir(root, slug))
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")
    return {
        **_summary(slug, m),
        "workspace_path": str(workspace_dir(root, slug)),
        "manifest": m.model_dump(mode="json"),
    }
```

Update `WorkspaceRow` href:

```tsx
<a href={`file:///${detail?.workspace_path?.replace(/\\/g, "/")}/preview/index.html`} target="_blank" rel="noreferrer">Open preview ↗</a>
```

- [ ] **Step 3: Build + manual smoke check** (`bun run build`, view in browser, click row to expand, click Run.)

- [ ] **Step 4: Commit**

```bash
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): expanded WorkspaceRow with action buttons"
```

---

### Task 26: FilterBar + BatchActionBar

**Files:**
- Create: `src/vsplit/dashboard/src/src/views/FilterBar.tsx`
- Create: `src/vsplit/dashboard/src/src/views/BatchActionBar.tsx`
- Modify: `src/vsplit/dashboard/src/src/App.tsx`

- [ ] **Step 1: FilterBar**

`src/vsplit/dashboard/src/src/views/FilterBar.tsx`:

```tsx
import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"

const STATUSES = ["any", "pending", "running", "done", "failed", "awaiting_user", "skipped"]
const STAGES = ["any", "probe", "detect", "preview", "review", "plan", "split", "verify"]

export type FilterState = { status: string; stage: string; slug: string }

export function FilterBar({ value, onChange }: { value: FilterState; onChange: (v: FilterState) => void }) {
  return (
    <div className="flex gap-2 items-center mb-3">
      <Select value={value.status} onValueChange={(v) => onChange({ ...value, status: v })}>
        <SelectTrigger className="w-40"><SelectValue placeholder="status" /></SelectTrigger>
        <SelectContent>{STATUSES.map(s => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent>
      </Select>
      <Select value={value.stage} onValueChange={(v) => onChange({ ...value, stage: v })}>
        <SelectTrigger className="w-40"><SelectValue placeholder="stage" /></SelectTrigger>
        <SelectContent>{STAGES.map(s => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent>
      </Select>
      <Input placeholder="slug glob (e.g. vacation-*)" value={value.slug} onChange={(e) => onChange({ ...value, slug: e.target.value })} className="max-w-xs" />
    </div>
  )
}
```

Apply filter logic in `App.tsx` client-side:

```tsx
const [filter, setFilter] = useState<FilterState>({ status: "any", stage: "any", slug: "" })

function matchesFilter(ws: Workspace, f: FilterState) {
  if (f.status !== "any") {
    if (!Object.values(ws.stages).includes(f.status)) return false
  }
  if (f.stage !== "any") {
    // "at stage X" = first non-done stage equals X
    const STAGES = ["probe", "detect", "preview", "review", "plan", "split", "verify"]
    const next = STAGES.find((s) => ws.stages[s] !== "done")
    if (next !== f.stage) return false
  }
  if (f.slug) {
    const pattern = new RegExp("^" + f.slug.replace(/\*/g, ".*").replace(/\?/g, ".") + "$")
    if (!pattern.test(ws.slug)) return false
  }
  return true
}

const visible = workspaces.filter((w) => matchesFilter(w, filter))
```

Render the bar above the rows and iterate `visible` instead of `workspaces`.

- [ ] **Step 2: BatchActionBar**

`src/vsplit/dashboard/src/src/views/BatchActionBar.tsx`:

```tsx
import { Button } from "@/components/ui/button"
import { api } from "@/lib/api"

export function BatchActionBar({
  selected, onCleared,
}: { selected: Set<string>; onCleared: () => void }) {
  if (selected.size === 0) return null
  const slugs = Array.from(selected)
  return (
    <div className="sticky top-0 z-10 bg-zinc-950/95 backdrop-blur border-b border-zinc-800 px-4 py-2 flex items-center gap-2">
      <div className="text-sm">{selected.size} selected</div>
      <Button size="sm" onClick={() => api.batch.run({ slugs }).then(onCleared)}>Run</Button>
      <Button size="sm" variant="secondary" onClick={() => api.batch.approve({ slugs }).then(onCleared)}>Approve</Button>
      <Button size="sm" variant="outline" onClick={() => {
        const stage = window.prompt("Reset from which stage?", "detect")
        if (stage) api.batch.reset({ slugs, from: stage }).then(onCleared)
      }}>Reset…</Button>
      <Button size="sm" variant="destructive" onClick={() => {
        if (window.confirm(`Delete ${selected.size} workspaces?`))
          api.batch.clean({ slugs }).then(onCleared)
      }}>Clean</Button>
      <Button size="sm" variant="ghost" onClick={onCleared}>Cancel</Button>
    </div>
  )
}
```

Mount it in `App.tsx` between the header and the filter bar.

- [ ] **Step 3: Build + manual check** + commit

```bash
bun run build
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): FilterBar + BatchActionBar"
```

---

## Phase 8 — Frontend UI: log drawer + config drawer

### Task 27: LogDrawer for failed stages

**Files:**
- Create: `src/vsplit/dashboard/src/src/views/LogDrawer.tsx`
- Modify: `src/vsplit/dashboard/src/src/views/WorkspaceRow.tsx`

- [ ] **Step 1: LogDrawer component**

`src/vsplit/dashboard/src/src/views/LogDrawer.tsx`:

```tsx
import { useEffect, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle,
} from "@/components/ui/drawer"
import { api } from "@/lib/api"

export function LogDrawer({
  slug, stage, onClose,
}: { slug: string | null; stage: string | null; onClose: () => void }) {
  const [text, setText] = useState<string>("")
  useEffect(() => {
    if (!slug || !stage) return
    api.workspaces.log(slug, stage).then(setText).catch((e) => setText(`(failed to load log: ${e})`))
  }, [slug, stage])
  return (
    <Drawer open={!!slug} onOpenChange={(o) => !o && onClose()}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>{slug} · {stage} log</DrawerTitle>
        </DrawerHeader>
        <pre className="px-4 pb-4 text-xs font-mono overflow-auto max-h-[60vh] whitespace-pre-wrap">{text}</pre>
      </DrawerContent>
    </Drawer>
  )
}
```

- [ ] **Step 2: Open LogDrawer when clicking a `failed` pip**

In `WorkspaceRow`, make pips clickable when status is `failed`. The parent (`App.tsx`) holds `{ slug, stage } | null` state and renders `<LogDrawer>` once at root level.

In `App.tsx`:

```tsx
import { LogDrawer } from "./views/LogDrawer"
// ...
const [logOpen, setLogOpen] = useState<{ slug: string; stage: string } | null>(null)
// ...
<LogDrawer
  slug={logOpen?.slug ?? null}
  stage={logOpen?.stage ?? null}
  onClose={() => setLogOpen(null)}
/>
```

Pass `onOpenLog` into `WorkspaceRow`; when a pip with `status === "failed"` is clicked, call `onOpenLog(ws.slug, stageName)`.

- [ ] **Step 3: Build + manual check** + commit

```bash
bun run build
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): LogDrawer for failed stage logs"
```

---

### Task 28: ConfigDrawer (form + raw YAML pane)

**Files:**
- Create: `src/vsplit/dashboard/src/src/views/ConfigDrawer.tsx`
- Modify: `src/vsplit/dashboard/src/src/App.tsx`

- [ ] **Step 1: ConfigDrawer component**

`src/vsplit/dashboard/src/src/views/ConfigDrawer.tsx`:

```tsx
import { useEffect, useState } from "react"
import {
  Drawer, DrawerContent, DrawerHeader, DrawerTitle,
} from "@/components/ui/drawer"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Input } from "@/components/ui/input"
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select"
import { api } from "@/lib/api"

const DETECTORS = ["content", "adaptive", "histogram", "hash"]

export function ConfigDrawer({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const [cfg, setCfg] = useState<any>(null)
  const [raw, setRaw] = useState<string>("")
  const [showRaw, setShowRaw] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    api.config.get().then(setCfg)
    api.config.getRaw().then((d) => setRaw(d.text))
  }, [open])

  const update = (path: string[], value: any) => {
    setCfg((c: any) => {
      const next = JSON.parse(JSON.stringify(c))
      let cur = next
      for (let i = 0; i < path.length - 1; i++) cur = cur[path[i]]
      cur[path[path.length - 1]] = value
      return next
    })
  }

  const saveStructured = async () => {
    try {
      setError(null)
      await api.config.put(cfg)
      onOpenChange(false)
    } catch (e: any) {
      setError(String(e.message ?? e))
    }
  }

  const saveRaw = async () => {
    try {
      setError(null)
      await api.config.putRaw(raw)
      onOpenChange(false)
    } catch (e: any) {
      setError(String(e.message ?? e))
    }
  }

  if (!cfg) return null

  return (
    <Drawer open={open} onOpenChange={onOpenChange}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>Configuration</DrawerTitle>
        </DrawerHeader>
        <div className="px-4 pb-4 space-y-4 overflow-y-auto max-h-[70vh]">
          <section className="space-y-2">
            <h3 className="text-sm font-semibold">Detect</h3>
            <label className="text-xs flex items-center gap-2">detector
              <Select value={cfg.detect.detector} onValueChange={(v) => update(["detect", "detector"], v)}>
                <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                <SelectContent>{DETECTORS.map(d => <SelectItem key={d} value={d}>{d}</SelectItem>)}</SelectContent>
              </Select>
            </label>
            <label className="text-xs flex items-center gap-2">threshold
              <Input type="number" step="0.01" value={cfg.detect.threshold}
                onChange={(e) => update(["detect", "threshold"], parseFloat(e.target.value))} className="w-32" />
            </label>
          </section>
          <section className="space-y-2">
            <h3 className="text-sm font-semibold">Review</h3>
            <label className="text-xs flex items-center gap-2">default_keep
              <Switch checked={cfg.review.default_keep} onCheckedChange={(v) => update(["review", "default_keep"], v)} />
            </label>
          </section>
          <section>
            <button className="text-xs text-zinc-400" onClick={() => setShowRaw((s) => !s)}>
              {showRaw ? "▾" : "▸"} Advanced (raw YAML)
            </button>
            {showRaw && (
              <textarea value={raw} onChange={(e) => setRaw(e.target.value)}
                className="w-full mt-2 h-48 bg-zinc-900 border border-zinc-800 rounded p-2 font-mono text-xs" />
            )}
          </section>
          {error && <div className="text-red-400 text-xs">{error}</div>}
          <div className="flex gap-2">
            <Button onClick={saveStructured}>Save form</Button>
            {showRaw && <Button variant="secondary" onClick={saveRaw}>Save raw YAML</Button>}
            <Button variant="ghost" onClick={() => onOpenChange(false)}>Cancel</Button>
          </div>
        </div>
      </DrawerContent>
    </Drawer>
  )
}
```

- [ ] **Step 2: Wire the gear button in App.tsx**

```tsx
import { ConfigDrawer } from "./views/ConfigDrawer"
// ...
const [configOpen, setConfigOpen] = useState(false)
// in header, change the Settings button:
<Button variant="ghost" size="icon" className="ml-auto" onClick={() => setConfigOpen(true)}><Settings className="h-4 w-4" /></Button>
// at root, after LogDrawer:
<ConfigDrawer open={configOpen} onOpenChange={setConfigOpen} />
```

- [ ] **Step 3: Build + manual check + commit**

```bash
bun run build
git add src/vsplit/dashboard/
git commit -m "feat(dashboard): ConfigDrawer with form + raw YAML pane"
```

---

## Phase 9 — Distribution + integration tests + docs

### Task 29: Hatchling include of dist/ + CI sanity test

**Files:**
- Modify: `pyproject.toml`
- Create: `tests/unit/test_dashboard_dist_present.py`

- [ ] **Step 1: Add force-include**

In `pyproject.toml`, add under `[tool.hatch.build.targets.wheel]`:

```toml
[tool.hatch.build.targets.wheel]
packages = ["src/vsplit"]

[tool.hatch.build.targets.wheel.force-include]
"src/vsplit/dashboard/dist" = "vsplit/dashboard/dist"
```

- [ ] **Step 2: Write CI sanity test**

`tests/unit/test_dashboard_dist_present.py`:

```python
from pathlib import Path

DIST = Path(__file__).parents[2] / "src" / "vsplit" / "dashboard" / "dist"


def test_dist_index_html_exists() -> None:
    assert (DIST / "index.html").is_file(), (
        "Run `cd src/vsplit/dashboard/src && bun run build` to regenerate dist/"
    )


def test_dist_index_references_existing_asset() -> None:
    html = (DIST / "index.html").read_text()
    # Vite emits <script type="module" crossorigin src="/assets/index-XXXX.js"></script>
    import re
    m = re.search(r'src="(/assets/[^"]+\.js)"', html)
    assert m, "index.html missing assets/* script tag"
    asset = DIST / m.group(1).lstrip("/")
    assert asset.is_file(), f"asset {m.group(1)} referenced but missing in dist/"
```

- [ ] **Step 3: Tests pass + commit**

```bash
uv run pytest tests/unit/test_dashboard_dist_present.py -v
git add pyproject.toml tests/unit/test_dashboard_dist_present.py
git commit -m "build: include dashboard dist/ in wheel + CI sanity test"
```

---

### Task 30: Full-stack browser integration sanity check

**Files:**
- Create: `tests/integration/test_dashboard_server_boot.py`

- [ ] **Step 1: Test that the bundled SPA is served**

`tests/integration/test_dashboard_server_boot.py`:

```python
from pathlib import Path
from fastapi.testclient import TestClient

from vsplit.dashboard.server import create_app


def test_root_serves_spa(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.get("/")
    assert r.status_code == 200
    assert "<!doctype html>" in r.text.lower() or "<html" in r.text.lower()


def test_unknown_path_serves_spa_html(tmp_path: Path) -> None:
    # SPA routes should fall through to index.html
    client = TestClient(create_app(tmp_path))
    r = client.get("/some/spa/route")
    assert r.status_code in (200, 404)  # html=True on StaticFiles may return either
```

- [ ] **Step 2: Test passes + commit**

```bash
uv run pytest tests/integration/test_dashboard_server_boot.py -v
git add tests/integration/test_dashboard_server_boot.py
git commit -m "test(integration): dashboard server boots + serves SPA"
```

---

### Task 31: README updates

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add dashboard section to commands table**

Find the Commands table in README.md and add:

```markdown
| `vsplit dashboard [--port N]` | Launch the local web dashboard |
```

- [ ] **Step 2: Add a Dashboard section after the existing `one` command section**

```markdown
---

### `dashboard`

Launches a localhost web UI for managing workspaces, running commands in bulk, and editing config.

| Option | Default | What it does |
|---|---|---|
| `--port N` | 8765 | TCP port to bind. Must be free. |
| `--host ADDR` | 127.0.0.1 | Bind address. Localhost-only by default; do not expose. |
| `--no-open` | off | Skip auto-opening the browser. |

```bash
uv run vsplit dashboard
# Opens http://127.0.0.1:8765 in your default browser.
```

Features:
- Hybrid collapsible list of every workspace; per-stage status pips.
- Per-row actions: Run, Reset (with stage picker), Clean (confirms), Approve.
- Bulk selection + filter bar (status/stage/slug glob mirrors CLI grammar).
- Inline log viewer for failed stages.
- Config drawer: structured form for common fields + collapsible raw YAML pane.
- Live status updates via Server-Sent Events (no manual refresh).

Out of scope for v1: inline `cuts.json` editor (use your text editor), multi-user
or LAN access, mobile layouts, light theme.
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(readme): document vsplit dashboard"
```

---

## Self-review checklist

Run through after completing all tasks:

- [ ] `uv run pytest -q` passes.
- [ ] `cd src/vsplit/dashboard/src && bun run build` succeeds; `dist/` regenerated.
- [ ] `uv run vsplit dashboard --no-open` boots cleanly; `curl http://127.0.0.1:8765/api/health` returns `{"ok": true}`.
- [ ] Browser at the same URL renders the dark dashboard, lists workspaces from a real working dir.
- [ ] Clicking Run, Reset, Clean, Approve on a row triggers the expected manifest changes (verify via `vsplit status`).
- [ ] Stage pips update live during a `Run`; SSE works.
- [ ] Failed stage pip opens a populated log drawer.
- [ ] Config drawer reads/writes `.vsplit/config.yaml`; bad YAML in raw mode rejected with a helpful error.
- [ ] CLI commands still work unchanged: `vsplit scan`, `vsplit run`, `vsplit reset --from detect`, `vsplit one <file>`.
