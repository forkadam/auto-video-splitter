import shutil
from datetime import datetime, timezone
from pathlib import Path
import pytest
from fastapi.testclient import TestClient

from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"


def _seed(tmp_path: Path, slug: str, *, awaiting=False, with_source=False) -> Path:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    src_rel = f"{slug}.mp4"
    if with_source and FIXTURE.exists():
        shutil.copy(FIXTURE, tmp_path / src_rel)
        size = (tmp_path / src_rel).stat().st_size
    else:
        size = 1
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=src_rel, size_bytes=size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    if awaiting:
        m.stages["review"].status = "awaiting_user"
        m.stages["review"].artifacts = {"cuts_file_hash": "x"}
    save_manifest(ws, m)
    # Accumulate into existing index
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=src_rel, size_bytes=size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)
    return ws


def test_clean_endpoint(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    r = TestClient(create_app(tmp_path)).post("/api/workspaces/alpha/clean")
    assert r.status_code == 200
    assert r.json() == {"cleaned": 1}
    assert not workspace_dir(tmp_path, "alpha").exists()


def test_approve_endpoint(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha", awaiting=True)
    r = TestClient(create_app(tmp_path)).post("/api/workspaces/alpha/approve")
    assert r.status_code == 200
    assert r.json() == {"approved": 1}
    m = load_manifest(workspace_dir(tmp_path, "alpha"))
    assert m.stages["review"].artifacts.get("approved_at")


def test_reset_endpoint(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    m = load_manifest(ws)
    m.stages["probe"].status = "done"
    m.stages["detect"].status = "done"
    save_manifest(ws, m)
    r = TestClient(create_app(tmp_path)).post(
        "/api/workspaces/alpha/reset", json={"from": "detect"}
    )
    assert r.status_code == 200
    m2 = load_manifest(ws)
    assert m2.stages["probe"].status == "done"
    assert m2.stages["detect"].status == "pending"


@pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")
def test_run_endpoint_advances_probe(tmp_path: Path) -> None:
    _seed(tmp_path, "short", with_source=True)
    r = TestClient(create_app(tmp_path)).post(
        "/api/workspaces/short/run", json={"only": "probe"}
    )
    assert r.status_code == 200
    m = load_manifest(workspace_dir(tmp_path, "short"))
    assert m.stages["probe"].status == "done"


def test_batch_clean(tmp_path: Path) -> None:
    _seed(tmp_path, "a")
    _seed(tmp_path, "b")
    _seed(tmp_path, "c")
    r = TestClient(create_app(tmp_path)).post(
        "/api/batch/clean", json={"slugs": ["a", "c"]}
    )
    assert r.status_code == 200
    assert r.json() == {"cleaned": 2}
    idx = load_index(tmp_path)
    assert "b" in idx.workspaces
    assert "a" not in idx.workspaces
    assert "c" not in idx.workspaces


def test_batch_approve(tmp_path: Path) -> None:
    _seed(tmp_path, "a", awaiting=True)
    _seed(tmp_path, "b", awaiting=False)
    r = TestClient(create_app(tmp_path)).post(
        "/api/batch/approve", json={"slugs": ["a", "b"]}
    )
    assert r.json() == {"approved": 1}
