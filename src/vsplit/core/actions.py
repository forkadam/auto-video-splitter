"""Callable action helpers shared by CLI commands and the dashboard API."""
from __future__ import annotations
import shutil
from datetime import datetime, timezone
from pathlib import Path

from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import load_manifest, save_manifest
from vsplit.core.paths import workspace_dir
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
