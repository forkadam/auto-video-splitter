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
