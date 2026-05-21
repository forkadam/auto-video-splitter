import shutil
from pathlib import Path
import pytest
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_one_runs_through_preview(tmp_path):
    target = tmp_path / "X.mp4"
    shutil.copy(FIXTURE, target)
    runner = CliRunner()
    result = runner.invoke(main, ["one", str(target), "--through", "preview"])
    assert result.exit_code == 0, result.output
    m = load_manifest(workspace_dir(tmp_path, "x"))
    assert m.stages["preview"].status == "done"
    assert m.stages["review"].status == "pending"

def test_one_auto_approve_completes(tmp_path):
    target = tmp_path / "Y.mp4"
    shutil.copy(FIXTURE, target)
    runner = CliRunner()
    result = runner.invoke(main, ["one", str(target), "--auto-approve"])
    assert result.exit_code == 0, result.output
    m = load_manifest(workspace_dir(tmp_path, "y"))
    assert m.stages["verify"].status == "done"
