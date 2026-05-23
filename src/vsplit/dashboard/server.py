"""FastAPI app factory + uvicorn launcher for the vsplit dashboard."""
from __future__ import annotations
import webbrowser
from pathlib import Path

import uvicorn
from fastapi import FastAPI


def create_app(working_root: Path) -> FastAPI:
    """Build a FastAPI app pointed at the given working root."""
    app = FastAPI(title="vsplit dashboard", version="0.1.0")
    app.state.working_root = working_root

    @app.get("/api/health")
    def health() -> dict:
        return {"ok": True}

    return app


def run_server(
    working_root: Path,
    *,
    host: str = "127.0.0.1",
    port: int = 8765,
    open_browser: bool = True,
) -> None:
    """Block on uvicorn until shutdown."""
    app = create_app(working_root)
    url = f"http://{host}:{port}"
    if open_browser:
        webbrowser.open(url)
    print(f"vsplit dashboard ready at {url}")
    uvicorn.run(app, host=host, port=port, log_level="info")
