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


@pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")
def test_run_publishes_started_and_finished(tmp_path: Path) -> None:
    shutil.copy(FIXTURE, tmp_path / FIXTURE.name)
    ws = workspace_dir(tmp_path, "short")
    ws.mkdir(parents=True)
    m = new_manifest(slug="short", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=(tmp_path / FIXTURE.name).stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = load_index(tmp_path)
    idx.workspaces["short"] = WorkspaceEntry(
        source_relative_path=FIXTURE.name,
        size_bytes=(tmp_path / FIXTURE.name).stat().st_size,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)

    app = create_app(tmp_path, heartbeat_interval=10.0)
    received: list[dict] = []

    # Use a real uvicorn server because TestClient.stream() blocks (see T9).
    import uvicorn, httpx, time, json, threading as th
    config = uvicorn.Config(app, host="127.0.0.1", port=18799, log_level="error")
    server = uvicorn.Server(config)
    t = th.Thread(target=server.run, daemon=True)
    t.start()
    try:
        # Wait for server up
        deadline = time.time() + 5.0
        with httpx.Client() as cl:
            while time.time() < deadline:
                try:
                    if cl.get("http://127.0.0.1:18799/api/health").status_code == 200:
                        break
                except Exception:
                    time.sleep(0.05)
            # Open SSE
            with cl.stream("GET", "http://127.0.0.1:18799/api/events", timeout=30.0) as r:
                # Trigger run in another thread
                def kick():
                    cl.post(
                        "http://127.0.0.1:18799/api/workspaces/short/run",
                        json={"only": "probe"},
                        timeout=30.0,
                    )
                t2 = th.Thread(target=kick)
                t2.start()
                for line in r.iter_lines():
                    if line.startswith("data:"):
                        payload = json.loads(line[5:].strip())
                        received.append(payload)
                        if payload.get("type") == "run_finished":
                            break
                t2.join()
    finally:
        server.should_exit = True
        t.join(timeout=5.0)

    types = [ev["type"] for ev in received]
    assert "run_started" in types
    assert "run_finished" in types
