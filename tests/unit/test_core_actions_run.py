import threading
from datetime import datetime, timezone
from pathlib import Path
import shutil
import pytest
from vsplit.core.actions import run_workspaces
from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.config import Config
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")


def test_run_workspaces_advances_probe(tmp_path: Path) -> None:
    target = tmp_path / FIXTURE.name
    shutil.copy(FIXTURE, target)
    ws = workspace_dir(tmp_path, "short")
    ws.mkdir(parents=True)
    m = new_manifest(slug="short", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=target.stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={"short": WorkspaceEntry(
        source_relative_path=FIXTURE.name, size_bytes=target.stat().st_size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))

    run_workspaces(tmp_path, ["short"], config=Config(), only="probe",
                   cancel_event=threading.Event())
    m2 = load_manifest(ws)
    assert m2.stages["probe"].status == "done"
