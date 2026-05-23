from datetime import datetime, timezone
from pathlib import Path
from fastapi.testclient import TestClient

from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed_with_log(tmp_path: Path, slug: str, stage: str, log_text: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    (ws / "logs").mkdir()
    (ws / "logs" / f"{stage}.log").write_text(log_text)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))


def test_log_endpoint_returns_text(tmp_path: Path) -> None:
    _seed_with_log(tmp_path, "alpha", "probe", "line one\nline two\n")
    r = TestClient(create_app(tmp_path)).get("/api/workspaces/alpha/logs/probe")
    assert r.status_code == 200
    assert r.text.startswith("line one")


def test_log_endpoint_404_for_missing(tmp_path: Path) -> None:
    _seed_with_log(tmp_path, "alpha", "probe", "x")
    r = TestClient(create_app(tmp_path)).get("/api/workspaces/alpha/logs/detect")
    assert r.status_code == 404


def test_log_endpoint_rejects_unknown_slug(tmp_path: Path) -> None:
    _seed_with_log(tmp_path, "alpha", "probe", "x")
    r = TestClient(create_app(tmp_path)).get("/api/workspaces/bogus/logs/probe")
    assert r.status_code == 404
