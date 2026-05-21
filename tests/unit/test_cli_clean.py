from datetime import datetime, timezone
from pathlib import Path
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str):
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


def test_clean_removes_workspace_and_retires_slug(tmp_path):
    _seed(tmp_path, "birthday")
    runner = CliRunner()
    result = runner.invoke(main, ["-w", str(tmp_path),
                                  "clean", "birthday", "--force"])
    assert result.exit_code == 0
    assert not workspace_dir(tmp_path, "birthday").exists()
    idx = load_index(tmp_path)
    assert "birthday" not in idx.workspaces
    assert "birthday" in idx.retired_slugs
