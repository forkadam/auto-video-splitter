"""Gather every workspace's `output/` clips into a single destination directory.

Why this isn't a stage:
    Stages own per-workspace state (status, recovery, output_subdir). Collection
    is a cross-cutting filesystem operation with nothing to track per-workspace
    and no useful "running" or "failed" semantics — `shutil.copy` either works
    or raises. Keeping it as a separate action means originals stay put unless
    the caller opts in, and re-collecting after destination/mode changes is a
    no-op idempotent re-run.
"""
from __future__ import annotations
import os
import shutil
from pathlib import Path
from typing import Iterable, Literal

from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir


CollectMode = Literal["copy", "move", "link"]

_VALID_MODES: frozenset[str] = frozenset({"copy", "move", "link"})


def _place(src: Path, dst: Path, mode: CollectMode) -> None:
    """Place *src* at *dst* under the chosen mode. Destination parent must exist."""
    if mode == "copy":
        # copy2 preserves mtime so the destination clip's timestamp matches
        # what split.py wrote — handy for sorting "collected" by capture date.
        shutil.copy2(src, dst)
    elif mode == "move":
        shutil.move(str(src), str(dst))
    elif mode == "link":
        # Try hardlink first (cheap, same volume). Fall back to symlink for
        # cross-volume; symlink on Windows needs Developer Mode enabled.
        try:
            os.link(src, dst)
        except OSError:
            os.symlink(src, dst)
    else:  # pragma: no cover — guarded by validate-on-entry
        raise ValueError(f"unknown collect mode: {mode!r}")


def collect_outputs(
    working_root: Path,
    *,
    slugs: Iterable[str] | None = None,
    dest: Path | None = None,
    mode: CollectMode = "copy",
    force: bool = False,
    dry_run: bool = False,
) -> dict:
    """Place every selected workspace's `output/*` files at *dest*.

    Args:
        working_root: the .vsplit root.
        slugs: limit to these workspace slugs; ``None`` means every workspace
            whose split stage is ``done``.
        dest: destination directory. Defaults to ``<working_root>/collected``.
        mode: ``copy`` (default — safest), ``move``, or ``link``.
        force: if True, overwrite existing files at the destination. Otherwise
            leave them alone and report them in ``skipped_existing``.
        dry_run: if True, returns the same counts as a real run would but does
            not touch the filesystem (no destination dir created, no files
            placed, no sources removed). Lets the dashboard preview the action
            before the user confirms.

    Returns:
        ``{collected, from_workspaces, skipped_not_done, skipped_no_output,
        skipped_existing, skipped_ignored, files}`` — ``files`` is the list of destination paths
        that were actually written/linked this call.
    """
    if mode not in _VALID_MODES:
        raise ValueError(f"unknown collect mode: {mode!r}; expected one of {sorted(_VALID_MODES)}")

    if dest is None:
        dest = working_root / "collected"
    if not dry_run:
        dest.mkdir(parents=True, exist_ok=True)

    collect_all = slugs is None
    idx = load_index(working_root)
    candidate_slugs = list(idx.workspaces.keys()) if slugs is None else list(slugs)

    collected = 0
    from_workspaces = 0
    skipped_not_done = 0
    skipped_no_output = 0
    skipped_existing = 0
    skipped_ignored = 0
    files: list[str] = []

    for slug in candidate_slugs:
        if slug not in idx.workspaces:
            continue
        ws = workspace_dir(working_root, slug)
        try:
            m = load_manifest(ws)
        except FileNotFoundError:
            continue

        if collect_all and m.ignored:
            skipped_ignored += 1
            continue

        if m.stages["split"].status != "done":
            skipped_not_done += 1
            continue

        out_dir = ws / "output"
        if not out_dir.is_dir():
            skipped_no_output += 1
            continue

        # Snapshot before iterating — `move` mode mutates the source directory.
        clips = sorted(p for p in out_dir.iterdir() if p.is_file())
        if not clips:
            skipped_no_output += 1
            continue

        any_placed = False
        for src in clips:
            target = dest / src.name
            if target.exists() and not force:
                skipped_existing += 1
                continue
            if not dry_run:
                if target.exists() and force:
                    target.unlink()
                _place(src, target, mode)
            files.append(str(target))
            collected += 1
            any_placed = True
        if any_placed:
            from_workspaces += 1

    return {
        "collected": collected,
        "from_workspaces": from_workspaces,
        "skipped_not_done": skipped_not_done,
        "skipped_no_output": skipped_no_output,
        "skipped_existing": skipped_existing,
        "skipped_ignored": skipped_ignored,
        "files": files,
    }
