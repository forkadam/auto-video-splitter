"""FastAPI app factory + uvicorn launcher for the vsplit dashboard."""
from __future__ import annotations
import asyncio
import json
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.responses import StreamingResponse

from vsplit.dashboard.sse import EventBroker, _SHUTDOWN_SENTINEL
from vsplit.dashboard.watcher import ManifestWatcher


def is_proactor_connection_reset(context: dict) -> bool:
    """True iff *context* is the noisy "Windows + Proactor + cancelled range
    request" pattern that we want to filter from asyncio's exception logger.

    Specifically: a ConnectionResetError raised inside
    ``_ProactorBasePipeTransport._call_connection_lost`` while asyncio is
    already tearing down the connection. The response was already delivered;
    the only thing left is to close the socket the remote has already RST'd,
    which Windows reports as WinError 10054. The traceback adds zero signal
    and fires every time the browser cancels a `<video>` range request.

    Any other ConnectionResetError (or any other exception inside
    _call_connection_lost) is left alone so real bugs still surface.
    """
    exc = context.get("exception")
    if not isinstance(exc, ConnectionResetError):
        return False
    # Match the handle by repr, which is stable across CPython versions:
    # "<Handle _ProactorBasePipeTransport._call_connection_lost()>"
    handle_repr = repr(context.get("handle") or "")
    return "_call_connection_lost" in handle_repr


def install_proactor_noise_filter(loop: asyncio.AbstractEventLoop) -> None:
    """Install a loop exception handler that drops the noise documented in
    :func:`is_proactor_connection_reset` and chains everything else through
    to the previous handler (or asyncio's default)."""
    previous = loop.get_exception_handler()

    def _handler(loop: asyncio.AbstractEventLoop, context: dict) -> None:
        if is_proactor_connection_reset(context):
            return
        if previous is not None:
            previous(loop, context)
        else:
            loop.default_exception_handler(context)

    loop.set_exception_handler(_handler)


def create_app(
    working_root: Path,
    *,
    heartbeat_interval: float = 15.0,
) -> FastAPI:
    app = FastAPI(title="vsplit dashboard", version="0.1.0")
    app.state.working_root = working_root
    broker = EventBroker()
    app.state.broker = broker
    watcher = ManifestWatcher(working_root, broker)
    app.state.watcher = watcher

    from vsplit.dashboard.api import workspaces as workspaces_api
    app.include_router(workspaces_api.router)

    from vsplit.dashboard.api import logs as logs_api
    app.include_router(logs_api.router)

    from vsplit.dashboard.api import config as config_api
    app.include_router(config_api.router)

    from vsplit.dashboard.api import actions as actions_api
    app.include_router(actions_api.router)
    app.include_router(actions_api.batch_router)

    # Set when the app begins shutting down. The SSE stream watches this so it
    # can return cleanly instead of forcing uvicorn to hit its graceful-shutdown
    # timeout and surface a CancelledError from listen_for_disconnect.
    #
    # NOTE: FastAPI's @on_event("shutdown") fires AFTER uvicorn waits for active
    # connections to drain — too late for long-lived SSE streams. To trigger this
    # event at the start of shutdown instead, callers should wrap Server.shutdown
    # via wire_shutdown_signal() below before running the server.
    app.state.shutdown_event = None  # asyncio.Event, lazily created in startup

    @app.on_event("startup")
    def _startup() -> None:
        watcher.start()
        app.state.shutdown_event = asyncio.Event()
        # Cancelled video range requests on Windows + Proactor surface as
        # harmless ConnectionResetError tracebacks. Filter them out at the
        # loop level so a tab close or video scrub doesn't spam the logs.
        try:
            install_proactor_noise_filter(asyncio.get_running_loop())
        except RuntimeError:
            # No running loop (shouldn't happen inside startup, but be safe).
            pass

    @app.on_event("shutdown")
    def _shutdown() -> None:
        watcher.stop()
        ev = app.state.shutdown_event
        if ev is not None:
            ev.set()

    @app.get("/api/health")
    def health() -> dict:
        return {"ok": True}

    async def _event_stream():
        # Single-`wait_for` shape: only ONE inner coroutine per iteration so
        # cancellation never leaves an orphan Task pending. Shutdown signal
        # arrives as a sentinel value in the broker queue (see EventBroker.close_all),
        # which the same wait_for receives — no second awaitable to race.
        subscriber = broker.subscribe()
        try:
            while True:
                try:
                    ev = await asyncio.wait_for(
                        anext(subscriber),
                        timeout=heartbeat_interval,
                    )
                except asyncio.TimeoutError:
                    yield f"data: {json.dumps({'type': 'heartbeat'})}\n\n"
                    continue
                if ev is _SHUTDOWN_SENTINEL:
                    return
                yield f"data: {json.dumps(ev)}\n\n"
        finally:
            await subscriber.aclose()

    @app.get("/api/events")
    async def events():
        return StreamingResponse(_event_stream(), media_type="text/event-stream")

    from fastapi.staticfiles import StaticFiles
    dist = Path(__file__).parent / "dist"
    if dist.is_dir():
        app.mount("/", StaticFiles(directory=str(dist), html=True), name="spa")

    return app


def wire_shutdown_signal(app: FastAPI, server: uvicorn.Server) -> None:
    """Wrap Server.shutdown so the app's shutdown_event fires *before* uvicorn
    begins waiting for active connections to drain. Long-lived endpoints (SSE)
    can then return cleanly on their own, avoiding the noisy force-cancel.

    Also injects a sentinel into every SSE subscriber's queue so their
    blocking ``await queue.get()`` returns immediately and they can exit
    without us needing to cancel their inner await."""
    original_shutdown = server.shutdown

    async def _shutdown_wrapping(*args, **kwargs):
        ev = getattr(app.state, "shutdown_event", None)
        if ev is not None:
            ev.set()
        broker = getattr(app.state, "broker", None)
        if broker is not None:
            broker.close_all()
        return await original_shutdown(*args, **kwargs)

    server.shutdown = _shutdown_wrapping  # type: ignore[method-assign]


def run_server(working_root: Path, *, host="127.0.0.1", port=8765, open_browser=True) -> None:
    app = create_app(working_root)
    url = f"http://{host}:{port}"
    if open_browser:
        webbrowser.open(url)
    print(f"vsplit dashboard ready at {url}")
    # Cap graceful-shutdown: browser SSE keep-alives never close on their own,
    # but wire_shutdown_signal lets the SSE generator exit cleanly before the
    # timeout fires so the cap is now mostly belt-and-suspenders.
    config = uvicorn.Config(
        app, host=host, port=port, log_level="info",
        timeout_graceful_shutdown=5,
    )
    server = uvicorn.Server(config)
    wire_shutdown_signal(app, server)
    server.run()
