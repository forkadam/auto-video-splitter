from __future__ import annotations
from pathlib import Path
from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import FileResponse

from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

router = APIRouter(prefix="/api/workspaces", tags=["workspaces"])


def _summary(slug: str, m) -> dict:
    statuses = [b.status for b in m.stages.values()]
    return {
        "slug": slug,
        "source": m.source.relative_path,
        "stages": {n: b.status for n, b in m.stages.items()},
        "any_failed": "failed" in statuses,
        "any_awaiting": "awaiting_user" in statuses,
        "all_done": all(s == "done" for s in statuses),
    }


@router.get("")
def list_workspaces(request: Request) -> dict:
    root: Path = request.app.state.working_root
    idx = load_index(root)
    items = []
    for slug in idx.workspaces:
        try:
            m = load_manifest(workspace_dir(root, slug))
        except FileNotFoundError:
            continue
        items.append(_summary(slug, m))
    return {"workspaces": items}


@router.get("/{slug}")
def get_workspace(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        m = load_manifest(workspace_dir(root, slug))
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")
    return {
        **_summary(slug, m),
        "manifest": m.model_dump(mode="json"),
    }


@router.get("/{slug}/thumbs/{name}")
def get_thumb(slug: str, name: str, request: Request):
    root: Path = request.app.state.working_root
    # Reject anything that would escape the thumbs dir.
    if "/" in name or "\\" in name or ".." in name:
        raise HTTPException(status_code=400, detail="invalid name")
    img = workspace_dir(root, slug) / "preview" / "images" / name
    if not img.is_file():
        raise HTTPException(status_code=404, detail="thumb not found")
    return FileResponse(img, media_type="image/jpeg")
