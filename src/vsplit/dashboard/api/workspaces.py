from __future__ import annotations
import json
import webbrowser
from pathlib import Path
from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import FileResponse

from vsplit.core.hash import content_hash
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.cuts import CutsJson

router = APIRouter(prefix="/api/workspaces", tags=["workspaces"])


def _require_slug(working_root: Path, slug: str) -> None:
    """Reject unknown slugs with 404, also rejects path-traversal attempts."""
    idx = load_index(working_root)
    if slug not in idx.workspaces:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")


def _list_thumbs(root: Path, slug: str) -> list[str]:
    """Sorted list of scene-thumbnail filenames in the workspace's preview dir."""
    img_dir = workspace_dir(root, slug) / "preview" / "images"
    if not img_dir.is_dir():
        return []
    return sorted(p.name for p in img_dir.iterdir()
                  if p.is_file() and p.suffix.lower() == ".jpg")


def _scene_thumbs(root: Path, slug: str, scene_ids: list[int]) -> dict[str, str]:
    """For each scene id, the first sorted thumbnail filename matching scene-{id:03d}-*.jpg."""
    img_dir = workspace_dir(root, slug) / "preview" / "images"
    if not img_dir.is_dir():
        return {}
    all_names = sorted(p.name for p in img_dir.iterdir()
                       if p.is_file() and p.suffix.lower() == ".jpg")
    out: dict[str, str] = {}
    for sid in scene_ids:
        prefix = f"scene-{sid:03d}-"
        for name in all_names:
            if name.startswith(prefix):
                out[str(sid)] = name
                break
    return out


def _summary(root: Path, slug: str, m) -> dict:
    statuses = [b.status for b in m.stages.values()]
    thumbs = _list_thumbs(root, slug)
    return {
        "slug": slug,
        "source": m.source.relative_path,
        "stages": {n: b.status for n, b in m.stages.items()},
        "any_failed": "failed" in statuses,
        "any_awaiting": "awaiting_user" in statuses,
        "all_done": all(s == "done" for s in statuses),
        "first_thumb": thumbs[0] if thumbs else None,
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
        items.append(_summary(root, slug, m))
    return {"workspaces": items}


@router.get("/{slug}")
def get_workspace(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    try:
        m = load_manifest(workspace_dir(root, slug))
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")
    return {
        **_summary(root, slug, m),
        "workspace_path": str(workspace_dir(root, slug)),
        "thumb_names": _list_thumbs(root, slug),
        "manifest": m.model_dump(mode="json"),
    }


@router.post("/{slug}/open-preview")
def open_preview(slug: str, request: Request) -> dict:
    """Open this workspace's preview HTML in the OS default browser.

    Browsers block click-navigation from http:// to file:// for security, so
    we hand the file URI to the OS shell handler from the server side.
    """
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    p = workspace_dir(root, slug) / "preview" / "index.html"
    if not p.is_file():
        raise HTTPException(status_code=404, detail="preview not generated yet")
    webbrowser.open(p.as_uri())
    return {"opened": True, "path": str(p)}


@router.get("/{slug}/cuts")
def get_cuts(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    cuts_path = workspace_dir(root, slug) / "decisions" / "cuts.json"
    if not cuts_path.is_file():
        raise HTTPException(status_code=404, detail="cuts.json not yet written")
    raw = cuts_path.read_text()
    parsed = CutsJson.model_validate_json(raw)
    scene_ids = [s.id for s in parsed.scenes]
    return {
        "cuts": json.loads(raw),
        "hash": content_hash(cuts_path),
        "scene_thumbs": _scene_thumbs(root, slug, scene_ids),
    }


@router.get("/{slug}/thumbs/{name}")
def get_thumb(slug: str, name: str, request: Request):
    root: Path = request.app.state.working_root
    # Reject anything that would escape the thumbs dir.
    if "/" in name or "\\" in name or ".." in name:
        raise HTTPException(status_code=400, detail="invalid name")
    _require_slug(root, slug)
    img = workspace_dir(root, slug) / "preview" / "images" / name
    if not img.is_file():
        raise HTTPException(status_code=404, detail="thumb not found")
    return FileResponse(img, media_type="image/jpeg")
