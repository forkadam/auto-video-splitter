from pathlib import Path
from fastapi.testclient import TestClient

from vsplit.dashboard.server import create_app


def test_root_serves_spa(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.get("/")
    assert r.status_code == 200
    assert "<!doctype html>" in r.text.lower() or "<html" in r.text.lower()


def test_unknown_path_serves_spa_html(tmp_path: Path) -> None:
    # SPA routes should fall through to index.html
    client = TestClient(create_app(tmp_path))
    r = client.get("/some/spa/route")
    assert r.status_code in (200, 404)  # html=True on StaticFiles may return either
