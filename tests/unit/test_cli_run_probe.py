import shutil
from pathlib import Path
import pytest
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_run_advances_probe(tmp_path):
    shutil.copy(FIXTURE, tmp_path / "short.mp4")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    result = runner.invoke(main, ["-w", str(tmp_path), "run", "--only", "probe"])
    assert result.exit_code == 0, result.output
    m = load_manifest(workspace_dir(tmp_path, "short"))
    assert m.stages["probe"].status == "done"
