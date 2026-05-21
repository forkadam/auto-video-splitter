from datetime import datetime, timezone
from pathlib import Path
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.index import save_index
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


def test_approve_sets_approved_at(tmp_path):
    _seed_awaiting(tmp_path, "birthday")
    runner = CliRunner()
    result = runner.invoke(main, ["-w", str(tmp_path), "approve", "birthday"])
    assert result.exit_code == 0
    m = load_manifest(workspace_dir(tmp_path, "birthday"))
    assert m.stages["review"].artifacts.get("approved_at")
