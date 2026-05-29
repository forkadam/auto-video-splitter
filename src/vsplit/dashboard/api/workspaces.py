from __future__ import annotations
import json
import webbrowser
from pathlib import Path
from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import FileResponse, JSONResponse
from pydantic import BaseModel, ValidationError

from vsplit.core.atomic import atomic_write_text
from vsplit.core.hash import content_hash
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.cuts import CutsJson
from vsplit.stages.review import _validate_ids

router = APIRouter(prefix="/api/workspaces", tags=["workspaces"])


class _CutsPutBody(BaseModel):
    cuts: dict
    expected_hash: str | None = None


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


_VIDEO_EXTS = {".mp4", ".mov", ".mkv", ".webm", ".avi", ".m4v"}


def _count_outputs(root: Path, slug: str) -> int:
    """Number of video clips currently in the workspace's output/ dir.

    Counts files on disk (not the split manifest artifact) so the badge stays
    truthful if clips are added or removed outside the pipeline.
    """
    out_dir = workspace_dir(root, slug) / "output"
    if not out_dir.is_dir():
        return 0
    return sum(1 for p in out_dir.iterdir()
               if p.is_file() and p.suffix.lower() in _VIDEO_EXTS)


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
    detect = m.stages.get("detect")
    # Expose the detector's scene count so the dashboard can distinguish
    # "zero scenes detected — dead end" from "scenes ready for review".
    # Stays None until detect completes so the frontend can render "unknown"
    # instead of misleadingly showing 0.
    scenes_found = (
        detect.artifacts.get("scenes_found")
        if detect is not None and detect.status == "done"
        else None
    )
    return {
        "slug": slug,
        "source": m.source.relative_path,
        "stages": {n: b.status for n, b in m.stages.items()},
        "any_failed": "failed" in statuses,
        "any_awaiting": "awaiting_user" in statuses,
        "all_done": all(s == "done" for s in statuses),
        "first_thumb": thumbs[0] if thumbs else None,
        "scenes_found": scenes_found,
        "output_count": _count_outputs(root, slug),
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


@router.get("/{slug}/preview/{path:path}")
def serve_preview_file(slug: str, path: str, request: Request):
    """Serve files from a workspace's preview/ directory over HTTP.

    Lets the dashboard embed the rendered preview index.html in an iframe
    alongside the workspace list (instead of opening a separate file:// window).
    Relative URLs inside index.html (images/*, source-proxy.mp4) resolve to
    the same route via the wildcard path segment.
    """
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    base = (workspace_dir(root, slug) / "preview").resolve()
    target = (base / path).resolve()
    # Path-traversal guard: target must remain inside the preview dir.
    if base != target and base not in target.parents:
        raise HTTPException(status_code=400, detail="invalid path")
    if not target.is_file():
        raise HTTPException(status_code=404, detail="preview file not found")
    return FileResponse(target)


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
    try:
        parsed = CutsJson.model_validate_json(raw)
    except ValidationError as e:
        # include_context=False drops non-JSON-serializable ValueError refs
        # that field_validators put into ctx.
        raise HTTPException(status_code=422, detail=e.errors(include_context=False)) from e
    scene_ids = [s.id for s in parsed.scenes]
    return {
        "cuts": json.loads(raw),
        "hash": content_hash(cuts_path),
        "scene_thumbs": _scene_thumbs(root, slug, scene_ids),
    }


@router.put("/{slug}/cuts")
def put_cuts(slug: str, body: _CutsPutBody, request: Request):
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    ws = workspace_dir(root, slug)
    cuts_path = ws / "decisions" / "cuts.json"
    scenes_path = ws / "detect" / "scenes.json"
    if not scenes_path.is_file():
        raise HTTPException(status_code=404, detail="detect/scenes.json missing")

    try:
        parsed = CutsJson.model_validate(body.cuts)
    except ValidationError as e:
        # include_context=False drops non-JSON-serializable ValueError refs
        # that field_validators put into ctx.
        raise HTTPException(status_code=422, detail=e.errors(include_context=False)) from e

    detected_ids = {s["id"] for s in json.loads(scenes_path.read_text())["scenes"]}
    try:
        _validate_ids(parsed, detected_ids)
    except ValueError as e:
        raise HTTPException(status_code=422, detail=str(e)) from e

    if body.expected_hash is not None and cuts_path.is_file():
        current = content_hash(cuts_path)
        if current != body.expected_hash:
            return JSONResponse(
                status_code=409,
                content={"detail": "cuts.json changed on disk", "current_hash": current},
            )

    atomic_write_text(
        cuts_path,
        json.dumps(parsed.model_dump(), indent=2) + "\n",
    )
    return {"hash": content_hash(cuts_path)}


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
