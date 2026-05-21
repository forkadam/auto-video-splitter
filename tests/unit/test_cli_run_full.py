import shutil
from pathlib import Path
import pytest
import json
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_full_pipeline_stops_at_review(tmp_path):
    shutil.copy(FIXTURE, tmp_path / "short.mp4")
    runner = CliRunner()
    assert runner.invoke(main, ["-w", str(tmp_path), "scan"]).exit_code == 0
    result = runner.invoke(main, ["-w", str(tmp_path), "run"])
    assert result.exit_code == 0, result.output
    m = load_manifest(workspace_dir(tmp_path, "short"))
    assert m.stages["probe"].status == "done"
    assert m.stages["detect"].status == "done"
    assert m.stages["preview"].status == "done"
    assert m.stages["review"].status == "awaiting_user"
    assert m.stages["plan"].status == "pending"

def test_full_pipeline_after_approve_completes(tmp_path):
    shutil.copy(FIXTURE, tmp_path / "short.mp4")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    runner.invoke(main, ["-w", str(tmp_path), "run"])
    runner.invoke(main, ["-w", str(tmp_path), "approve", "short"])
    result = runner.invoke(main, ["-w", str(tmp_path), "run"])
    assert result.exit_code == 0, result.output
    m = load_manifest(workspace_dir(tmp_path, "short"))
    assert m.stages["plan"].status == "done"
    assert m.stages["split"].status == "done"
    assert m.stages["verify"].status == "done"
