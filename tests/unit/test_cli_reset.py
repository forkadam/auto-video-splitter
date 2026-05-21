from datetime import datetime, timezone
from pathlib import Path
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path):
    slug = "birthday"
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    (ws / "probe").mkdir()
    (ws / "probe" / "ffprobe.json").write_text("{}")
    (ws / "detect").mkdir()
    (ws / "detect" / "scenes.json").write_text("{}")
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path="birthday.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    m.stages["probe"].status = "done"
    m.stages["detect"].status = "done"
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path="birthday.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))


def test_reset_from_detect(tmp_path):
    _seed(tmp_path)
    runner = CliRunner()
    result = runner.invoke(main, ["-w", str(tmp_path),
                                  "reset", "birthday", "--from", "detect"])
    assert result.exit_code == 0, result.output
    m = load_manifest(workspace_dir(tmp_path, "birthday"))
    assert m.stages["probe"].status == "done"      # untouched
    assert m.stages["detect"].status == "pending"
    assert not (workspace_dir(tmp_path, "birthday") / "detect").exists()


def test_reset_all_clears_everything(tmp_path):
    _seed(tmp_path)
    runner = CliRunner()
    result = runner.invoke(main, ["-w", str(tmp_path),
                                  "reset", "birthday", "--all", "--force"])
    assert result.exit_code == 0
    m = load_manifest(workspace_dir(tmp_path, "birthday"))
    assert m.stages["probe"].status == "pending"
    assert m.stages["detect"].status == "pending"
