from __future__ import annotations
from pathlib import Path
from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import PlainTextResponse

from vsplit.core.index import load_index
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

router = APIRouter(prefix="/api/workspaces", tags=["logs"])


def _require_slug(working_root: Path, slug: str) -> None:
    idx = load_index(working_root)
    if slug not in idx.workspaces:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")


@router.get("/{slug}/logs/{stage}", response_class=PlainTextResponse)
def get_log(slug: str, stage: str, request: Request) -> str:
    if stage not in STAGE_NAMES:
        raise HTTPException(status_code=400, detail=f"unknown stage: {stage}")
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    p = workspace_dir(root, slug) / "logs" / f"{stage}.log"
    if not p.is_file():
        raise HTTPException(status_code=404, detail="log not found")
    return p.read_text(encoding="utf-8", errors="replace")
