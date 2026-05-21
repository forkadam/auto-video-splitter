"""Single-file workspace adoption — register one file without walking the folder.

Used by `vsplit one`, which targets a single clip. Also intended to be usable
from `vsplit scan` to keep the adoption logic in one place (not yet wired in).
"""
from __future__ import annotations
from datetime import datetime, timezone
from pathlib import Path

from vsplit.core.hash import partial_hash
from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import load_manifest, new_manifest, save_manifest
from vsplit.core.paths import working_state_dir, workspace_dir
from vsplit.core.slugify import resolve_collision, slugify
from vsplit.schemas.index import WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def adopt_or_find(working_root: Path, file_path: Path) -> str:
    """Ensure ``file_path`` is registered in the index. Return its slug.

    If a workspace already exists matching the file's partial hash, reuse it
    (updating the path pointer if the file was renamed). Otherwise create
    a new workspace + manifest + index entry.
    """
    rel = str(file_path.relative_to(working_root))
    stat = file_path.stat()
    ph = partial_hash(file_path)

    idx = load_index(working_root)
    by_hash = {w.partial_hash: slug for slug, w in idx.workspaces.items()}

    if ph in by_hash:
        existing_slug = by_hash[ph]
        entry = idx.workspaces[existing_slug]
        if entry.source_relative_path != rel:
            entry.source_relative_path = rel
            ws = workspace_dir(working_root, existing_slug)
            m = load_manifest(ws)
            m.source.relative_path = rel
            save_manifest(ws, m)
            save_index(working_root, idx)
        return existing_slug

    base = slugify(file_path.name)
    if not base:
        raise ValueError(f"cannot slugify {rel}")
    new_slug = resolve_collision(
        base,
        set(idx.workspaces.keys()),
        set(idx.retired_slugs),
    )
    ws = workspace_dir(working_root, new_slug)
    ws.mkdir(parents=True, exist_ok=True)
    source = SourceInfo(
        relative_path=rel,
        size_bytes=stat.st_size,
        mtime=datetime.fromtimestamp(stat.st_mtime, tz=timezone.utc),
        partial_hash=ph,
    )
    m = new_manifest(slug=new_slug, source=source)
    save_manifest(ws, m)
    idx.workspaces[new_slug] = WorkspaceEntry(
        source_relative_path=rel,
        size_bytes=stat.st_size,
        partial_hash=ph,
        adopted_at=datetime.now(timezone.utc),
    )
    working_state_dir(working_root).mkdir(parents=True, exist_ok=True)
    save_index(working_root, idx)
    return new_slug
