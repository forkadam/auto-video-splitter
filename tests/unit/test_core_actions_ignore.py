from datetime import datetime, timezone
from pathlib import Path

from vsplit.core.actions import ignore_workspaces, unignore_workspaces
from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> Path:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)
    return ws


def test_ignore_sets_flag_and_timestamp(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    count = ignore_workspaces(tmp_path, ["alpha"])
    assert count == 1
    m = load_manifest(ws)
    assert m.ignored is True
    assert m.ignored_at is not None


def test_ignore_is_idempotent(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    ignore_workspaces(tmp_path, ["alpha"])
    assert ignore_workspaces(tmp_path, ["alpha"]) == 0


def test_unignore_clears_flag_and_timestamp(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    ignore_workspaces(tmp_path, ["alpha"])
    count = unignore_workspaces(tmp_path, ["alpha"])
    assert count == 1
    m = load_manifest(ws)
    assert m.ignored is False
    assert m.ignored_at is None


def test_unignore_on_not_ignored_is_noop(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    assert unignore_workspaces(tmp_path, ["alpha"]) == 0


def test_ignore_skips_missing_slugs(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    assert ignore_workspaces(tmp_path, ["alpha", "ghost"]) == 1
