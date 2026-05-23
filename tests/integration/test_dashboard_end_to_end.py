import json
import shutil
import threading
from datetime import datetime, timezone
from pathlib import Path
import pytest
from fastapi.testclient import TestClient

from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")


def test_full_workflow_via_api(tmp_path: Path) -> None:
    """Adopt → run preview → approve → run rest → clean — all via the API."""
    src = tmp_path / FIXTURE.name
    shutil.copy(FIXTURE, src)
    ws = workspace_dir(tmp_path, "short")
    ws.mkdir(parents=True)
    m = new_manifest(slug="short", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={"short": WorkspaceEntry(
        source_relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))

    app = create_app(tmp_path)
    with TestClient(app) as client:
        # Run through review (preview runs first, then review parks at awaiting_user)
        r = client.post("/api/workspaces/short/run", json={"through": "review"})
        assert r.status_code == 200
        # Detail shows review awaiting_user
        detail = client.get("/api/workspaces/short").json()
        assert detail["stages"]["preview"] == "done"
        assert detail["stages"]["review"] == "awaiting_user"
        # Approve
        r = client.post("/api/workspaces/short/approve")
        assert r.json() == {"approved": 1}
        # Finish pipeline
        r = client.post("/api/workspaces/short/run", json={})
        assert r.status_code == 200
        detail = client.get("/api/workspaces/short").json()
        assert detail["stages"]["verify"] == "done"
        assert detail["all_done"] is True
        # Clean
        r = client.post("/api/workspaces/short/clean")
        assert r.json() == {"cleaned": 1}
        assert "short" not in load_index(tmp_path).workspaces
