"""Tests for the /api/events SSE endpoint.

starlette.testclient.TestClient buffers the entire HTTP response before
returning (portal.call blocks until the ASGI coroutine finishes).  An
infinite SSE generator never finishes, so TestClient is incompatible with
infinite SSE streams.  We therefore spin up a real uvicorn server in a
daemon thread, hit it with a real httpx client that truly streams, and
stop the server after the first heartbeat is received.
"""
import asyncio
import json
import threading
import time
from pathlib import Path

import httpx
import pytest
import uvicorn

from vsplit.dashboard.server import create_app


def _start_server(app, host: str, port: int) -> uvicorn.Server:
    """Start *app* on *host*:*port* in a daemon thread; return the Server."""
    config = uvicorn.Config(app, host=host, port=port, log_level="error")
    server = uvicorn.Server(config)
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    # Wait up to 5 s for the server to become reachable.
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        time.sleep(0.05)
        try:
            httpx.get(f"http://{host}:{port}/api/health", timeout=0.5)
            break
        except Exception:
            pass
    return server


def test_events_endpoint_yields_heartbeat(tmp_path: Path) -> None:
    app = create_app(working_root=tmp_path, heartbeat_interval=0.05)
    server = _start_server(app, "127.0.0.1", 19876)
    try:
        with httpx.Client() as client:
            with client.stream("GET", "http://127.0.0.1:19876/api/events", timeout=10) as response:
                assert response.status_code == 200
                assert response.headers["content-type"].startswith("text/event-stream")
                data_lines = []
                for line in response.iter_lines():
                    if line.startswith("data:"):
                        data_lines.append(line)
                    if len(data_lines) >= 1:
                        break
                assert any("heartbeat" in line for line in data_lines)
    finally:
        server.should_exit = True
