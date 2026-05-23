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
