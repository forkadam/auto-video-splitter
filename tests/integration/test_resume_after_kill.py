import shutil
from pathlib import Path
import pytest
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.manifest import load_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import StageBlock

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_running_status_recovers_on_next_run(tmp_path):
    shutil.copy(FIXTURE, tmp_path / "short.mp4")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    # Simulate a crash by manually setting a stage status to running
    ws = workspace_dir(tmp_path, "short")
    runner.invoke(main, ["-w", str(tmp_path), "run", "--only", "probe"])
    m = load_manifest(ws)
    m.stages["probe"].status = "running"      # pretend we crashed mid-probe
    # also drop a leftover artifact
    (ws / "probe" / "partial.tmp").write_text("partial")
    save_manifest(ws, m)
    # Next run should recover (delete partial, reset to pending, re-run)
    result = runner.invoke(main, ["-w", str(tmp_path), "run", "--only", "probe"])
    assert result.exit_code == 0
    m = load_manifest(ws)
    assert m.stages["probe"].status == "done"
    assert not (ws / "probe" / "partial.tmp").exists()
