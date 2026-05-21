import shutil
from pathlib import Path
import pytest
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.core.index import load_index

FIXTURE_MP4 = Path(__file__).parents[1] / "fixtures" / "short.mp4"
FIXTURE_MKV = Path(__file__).parents[1] / "fixtures" / "short.mkv"

pytestmark = pytest.mark.skipif(
    not (FIXTURE_MP4.exists() and FIXTURE_MKV.exists()),
    reason="run tests/fixtures/make_fixtures.py first",
)

def test_three_file_batch_with_one_failure(tmp_path):
    shutil.copy(FIXTURE_MP4, tmp_path / "Birthday.mp4")
    shutil.copy(FIXTURE_MKV, tmp_path / "Hike.mkv")
    # Corrupt third file so it fails detect
    (tmp_path / "Broken.mp4").write_bytes(b"not a video")
    runner = CliRunner()
    assert runner.invoke(main, ["-w", str(tmp_path), "scan"]).exit_code == 0
    runner.invoke(main, ["-w", str(tmp_path), "run", "--through", "review"])
    idx = load_index(tmp_path)
    statuses = {}
    for slug in idx.workspaces:
        m = load_manifest(workspace_dir(tmp_path, slug))
        statuses[slug] = {
            "review": m.stages["review"].status,
            "any_failed": any(b.status == "failed" for b in m.stages.values()),
        }
    # Two workspaces parked at review; broken one failed somewhere
    awaiting = sum(1 for s in statuses.values() if s["review"] == "awaiting_user")
    failed = sum(1 for s in statuses.values() if s["any_failed"])
    assert awaiting == 2
    assert failed == 1
