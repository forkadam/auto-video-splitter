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


def test_one_does_not_adopt_siblings(tmp_path):
    """`vsplit one` must NOT walk the folder and pick up neighbouring files."""
    target = tmp_path / "Target.mp4"
    sibling_a = tmp_path / "Sibling_A.mp4"
    sibling_b = tmp_path / "Sibling_B.mov"
    shutil.copy(FIXTURE, target)
    shutil.copy(FIXTURE, sibling_a)
    shutil.copy(FIXTURE, sibling_b)
    runner = CliRunner()
    result = runner.invoke(main, ["one", str(target), "--through", "probe"])
    assert result.exit_code == 0, result.output
    from vsplit.core.index import load_index
    idx = load_index(tmp_path)
    # Only the target was adopted; siblings were ignored.
    assert "target" in idx.workspaces
    assert "sibling-a" not in idx.workspaces
    assert "sibling-b" not in idx.workspaces
    assert len(idx.workspaces) == 1
