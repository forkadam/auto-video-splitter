"""FastAPI app factory + uvicorn launcher for the vsplit dashboard."""
from __future__ import annotations
import asyncio
import json
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.responses import StreamingResponse

from vsplit.dashboard.sse import EventBroker
from vsplit.dashboard.watcher import ManifestWatcher


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

    @app.on_event("startup")
    def _startup() -> None:
        watcher.start()

    @app.on_event("shutdown")
    def _shutdown() -> None:
        watcher.stop()

    @app.get("/api/health")
    def health() -> dict:
        return {"ok": True}

    async def _event_stream():
        subscriber = broker.subscribe()
        last = asyncio.get_event_loop().time()
        try:
            while True:
                try:
                    ev = await asyncio.wait_for(
                        anext(subscriber),
                        timeout=heartbeat_interval,
                    )
                    yield f"data: {json.dumps(ev)}\n\n"
                except asyncio.TimeoutError:
                    yield f"data: {json.dumps({'type': 'heartbeat'})}\n\n"
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


def run_server(working_root: Path, *, host="127.0.0.1", port=8765, open_browser=True) -> None:
    app = create_app(working_root)
    url = f"http://{host}:{port}"
    if open_browser:
        webbrowser.open(url)
    print(f"vsplit dashboard ready at {url}")
    # Cap graceful-shutdown: browser SSE keep-alives never close on their own,
    # so Ctrl+C would otherwise wait forever for the /api/events stream.
    uvicorn.run(
        app, host=host, port=port, log_level="info",
        timeout_graceful_shutdown=5,
    )
