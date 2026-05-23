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


def test_workspace_lists_thumbs_when_preview_dir_exists(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    img_dir = workspace_dir(tmp_path, "alpha") / "preview" / "images"
    img_dir.mkdir(parents=True)
    (img_dir / "scene-001-01.jpg").write_bytes(b"\xff\xd8\xff\xe0a")
    (img_dir / "scene-001-02.jpg").write_bytes(b"\xff\xd8\xff\xe0b")
    (img_dir / "scene-002-01.jpg").write_bytes(b"\xff\xd8\xff\xe0c")
    client = TestClient(create_app(tmp_path))

    # list response carries first_thumb
    r = client.get("/api/workspaces")
    entry = next(w for w in r.json()["workspaces"] if w["slug"] == "alpha")
    assert entry["first_thumb"] == "scene-001-01.jpg"

    # detail response carries the full sorted thumb list
    r = client.get("/api/workspaces/alpha")
    assert r.json()["thumb_names"] == [
        "scene-001-01.jpg", "scene-001-02.jpg", "scene-002-01.jpg",
    ]


def test_workspace_no_thumbs_when_preview_dir_missing(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    assert client.get("/api/workspaces").json()["workspaces"][0]["first_thumb"] is None
    assert client.get("/api/workspaces/alpha").json()["thumb_names"] == []


def test_open_preview_invokes_webbrowser(tmp_path: Path, monkeypatch) -> None:
    _seed(tmp_path, "alpha")
    preview = workspace_dir(tmp_path, "alpha") / "preview"
    preview.mkdir(parents=True)
    (preview / "index.html").write_text("<html></html>")
    opened: list[str] = []
    import webbrowser as _wb
    monkeypatch.setattr(_wb, "open", lambda url: opened.append(url) or True)
    # Server captures `webbrowser` at import time; patch the one in the API module too.
    from vsplit.dashboard.api import workspaces as workspaces_mod
    monkeypatch.setattr(workspaces_mod.webbrowser, "open",
                        lambda url: opened.append(url) or True)
    r = TestClient(create_app(tmp_path)).post("/api/workspaces/alpha/open-preview")
    assert r.status_code == 200
    assert r.json()["opened"] is True
    assert len(opened) == 1
    assert "preview/index.html" in opened[0].replace("\\", "/")


def test_open_preview_404_when_missing(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    r = TestClient(create_app(tmp_path)).post("/api/workspaces/alpha/open-preview")
    assert r.status_code == 404


def test_workspace_detail_rejects_path_traversal(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    client = TestClient(create_app(tmp_path))
    # Try to escape via slug
    r = client.get("/api/workspaces/..%2F..%2Fescape")
    assert r.status_code in (404, 400)
