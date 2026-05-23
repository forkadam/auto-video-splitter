from datetime import datetime, timezone
from pathlib import Path
from fastapi.testclient import TestClient

from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    # Accumulate into the existing index rather than overwriting.
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)


def test_list_workspaces(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    _seed(tmp_path, "beta")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces")
    assert r.status_code == 200
    slugs = [w["slug"] for w in r.json()["workspaces"]]
    assert sorted(slugs) == ["alpha", "beta"]


def test_workspace_detail(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha")
    assert r.status_code == 200
    assert r.json()["slug"] == "alpha"
    assert "stages" in r.json()


def test_workspace_detail_404(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/missing")
    assert r.status_code == 404


def test_thumb_serving(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    img_dir = workspace_dir(tmp_path, "alpha") / "preview" / "images"
    img_dir.mkdir(parents=True)
    (img_dir / "scene-001-01.jpg").write_bytes(b"\xff\xd8\xff\xe0fake")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/thumbs/scene-001-01.jpg")
    assert r.status_code == 200
    assert r.content == b"\xff\xd8\xff\xe0fake"


def test_thumb_404(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/thumbs/missing.jpg")
    assert r.status_code == 404


def test_thumb_traversal_rejected(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/thumbs/../../escape.jpg")
    assert r.status_code in (400, 404)


def test_workspace_detail_rejects_path_traversal(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    # Try to escape via slug
    r = client.get("/api/workspaces/..%2F..%2Fescape")
    assert r.status_code in (404, 400)
