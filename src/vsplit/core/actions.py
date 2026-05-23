"""Callable action helpers shared by CLI commands and the dashboard API."""
from __future__ import annotations
import shutil
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import load_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import STAGE_NAMES, StageBlock


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
    from vsplit.core.orchestrator import Workspace, dispatch

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
