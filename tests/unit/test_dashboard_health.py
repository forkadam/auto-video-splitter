from pathlib import Path
from fastapi.testclient import TestClient
from vsplit.dashboard.server import create_app


def test_health_endpoint_returns_ok(tmp_path: Path) -> None:
    app = create_app(working_root=tmp_path)
    client = TestClient(app)
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json() == {"ok": True}
