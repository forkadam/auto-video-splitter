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
