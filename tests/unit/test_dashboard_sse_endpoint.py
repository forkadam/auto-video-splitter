"""Tests for the /api/events SSE endpoint.

starlette.testclient.TestClient buffers the entire HTTP response before
returning (portal.call blocks until the ASGI coroutine finishes).  An
infinite SSE generator never finishes, so TestClient is incompatible with
infinite SSE streams.  We therefore spin up a real uvicorn server in a
daemon thread, hit it with a real httpx client that truly streams, and
stop the server after the first heartbeat is received.
"""
import asyncio
import gc
import io
import json
import logging
import threading
import time
from pathlib import Path

import httpx
import pytest
import uvicorn

from vsplit.dashboard.server import (
    create_app,
    is_proactor_connection_reset,
    wire_shutdown_signal,
)


def _start_server(app, host: str, port: int, *, wire_shutdown: bool = True) -> uvicorn.Server:
    """Start *app* on *host*:*port* in a daemon thread; return the Server."""
    config = uvicorn.Config(app, host=host, port=port, log_level="error")
    server = uvicorn.Server(config)
    if wire_shutdown:
        wire_shutdown_signal(app, server)
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


def test_events_stream_exits_promptly_on_shutdown(tmp_path: Path) -> None:
    """The SSE stream awaits the broker indefinitely. Without a shutdown signal
    the generator never returns, uvicorn force-cancels the task after its
    graceful-shutdown timeout, and CancelledError leaks out as 'ERROR: Exception
    in ASGI application' noise. The stream must observe the app's shutdown event
    and return on its own."""
    app = create_app(working_root=tmp_path, heartbeat_interval=60.0)
    server = _start_server(app, "127.0.0.1", 19877)
    try:
        with httpx.Client() as client:
            with client.stream("GET", "http://127.0.0.1:19877/api/events", timeout=10) as response:
                assert response.status_code == 200
                # Trigger shutdown after the client has begun streaming.
                server.should_exit = True
                # The stream must end within a couple of seconds even though
                # the heartbeat interval is 60s — the shutdown event short-
                # circuits the inner wait_for.
                t0 = time.monotonic()
                lines = list(response.iter_lines())  # blocks until the server closes the stream
                elapsed = time.monotonic() - t0
        assert elapsed < 3.0, (
            f"SSE stream took {elapsed:.1f}s to close — shutdown event isn't being observed, "
            f"so uvicorn is force-cancelling after its grace timeout"
        )
        # Sanity: the loop didn't crash before yielding anything; lines may be empty if
        # shutdown beat the first heartbeat, but the stream must have closed cleanly.
        assert isinstance(lines, list)
    finally:
        # already requested above; ensure thread joins
        server.should_exit = True


def test_proactor_connection_reset_filter() -> None:
    """The asyncio loop exception handler must suppress ConnectionResetError
    coming out of _ProactorBasePipeTransport._call_connection_lost — that's the
    harmless tail of every cancelled browser range request on Windows. Other
    ConnectionResetErrors (and any non-reset exception) must still fall through."""

    # Real-shape context from the user's traceback: exception is a ConnectionResetError
    # and the failing handle is _call_connection_lost.
    class _FakeHandle:
        def __repr__(self) -> str:
            return "<Handle _ProactorBasePipeTransport._call_connection_lost()>"
    proactor_ctx = {
        "message": "Exception in callback _ProactorBasePipeTransport._call_connection_lost()",
        "exception": ConnectionResetError(10054, "forcibly closed"),
        "handle": _FakeHandle(),
    }
    assert is_proactor_connection_reset(proactor_ctx) is True

    # Same exception type but unrelated context → must NOT be suppressed.
    other_ctx = {
        "message": "Fatal error on transport",
        "exception": ConnectionResetError(10054, "forcibly closed"),
        "handle": None,
    }
    assert is_proactor_connection_reset(other_ctx) is False

    # Different exception in the same _call_connection_lost handle → not our target.
    runtime_ctx = {
        "message": "Exception in callback _ProactorBasePipeTransport._call_connection_lost()",
        "exception": RuntimeError("something else"),
        "handle": _FakeHandle(),
    }
    assert is_proactor_connection_reset(runtime_ctx) is False

    # Empty context: no exception → must NOT suppress.
    assert is_proactor_connection_reset({}) is False


def test_sse_disconnect_does_not_leak_pending_tasks(tmp_path: Path) -> None:
    """When a browser drops the SSE connection (page reload, tab close, auto-
    reconnect after the EventSource times out), Starlette cancels the response
    task. If _event_stream had created any sibling Tasks during that iteration,
    cancellation could leave them in 'cancel scheduled but not yet processed'
    state — asyncio's logger prints 'Task was destroyed but it is pending!' at
    GC time. This test reproduces the reconnect pattern and asserts the
    asyncio logger stays clean."""
    # Capture asyncio's logger output for the duration of the test.
    buf = io.StringIO()
    handler = logging.StreamHandler(buf)
    handler.setLevel(logging.WARNING)
    asyncio_logger = logging.getLogger("asyncio")
    prior_level = asyncio_logger.level
    asyncio_logger.addHandler(handler)
    asyncio_logger.setLevel(logging.WARNING)

    app = create_app(working_root=tmp_path, heartbeat_interval=0.05)
    server = _start_server(app, "127.0.0.1", 19878)
    try:
        # Open and abruptly close several SSE streams to simulate the
        # browser reconnect cycle that triggered the user-reported leak.
        for _ in range(5):
            with httpx.Client() as client:
                with client.stream(
                    "GET", "http://127.0.0.1:19878/api/events", timeout=2
                ) as response:
                    assert response.status_code == 200
                    # Read at least one event line so the generator is mid-flight,
                    # then drop the connection by exiting the context.
                    for line in response.iter_lines():
                        if line.startswith("data:"):
                            break
            # Give the server loop a beat to process the cancellation.
            time.sleep(0.1)
        # Force a GC sweep so any leaked tasks would trigger their warning now.
        for _ in range(3):
            gc.collect()
        time.sleep(0.2)
        gc.collect()
    finally:
        server.should_exit = True
        asyncio_logger.removeHandler(handler)
        asyncio_logger.setLevel(prior_level)

    log_output = buf.getvalue()
    assert "Task was destroyed but it is pending" not in log_output, (
        f"SSE generator leaked pending tasks during disconnect/reconnect:\n{log_output}"
    )
