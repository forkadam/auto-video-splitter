from pathlib import Path
from fastapi.testclient import TestClient
from vsplit.dashboard.server import create_app


def test_get_config_returns_defaults(tmp_path: Path) -> None:
    r = TestClient(create_app(tmp_path)).get("/api/config")
    assert r.status_code == 200
    cfg = r.json()
    assert cfg["detect"]["threshold"] == 27.0
    assert cfg["detect"]["detector"] == "content"


def test_get_config_raw_empty_when_no_file(tmp_path: Path) -> None:
    r = TestClient(create_app(tmp_path)).get("/api/config/raw")
    assert r.status_code == 200
    assert r.json()["text"] == ""


def test_get_config_raw_returns_file_content(tmp_path: Path) -> None:
    (tmp_path / ".vsplit").mkdir()
    (tmp_path / ".vsplit" / "config.yaml").write_text("detect:\n  threshold: 35\n")
    r = TestClient(create_app(tmp_path)).get("/api/config/raw")
    assert "threshold: 35" in r.json()["text"]


def test_get_config_schema_has_detect_section(tmp_path: Path) -> None:
    r = TestClient(create_app(tmp_path)).get("/api/config/schema")
    assert r.status_code == 200
    schema = r.json()
    # Pydantic JSON-schema nests via $defs / properties; just check Detect appears.
    assert "DetectConfig" in str(schema)
