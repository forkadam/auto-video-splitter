from __future__ import annotations
import threading
from pathlib import Path

from fastapi import APIRouter, HTTPException, Request
from fastapi.concurrency import run_in_threadpool
from pydantic import BaseModel, Field

from vsplit.core.actions import (
    approve_workspaces, clean_workspaces, reset_workspaces, run_workspaces,
)
from vsplit.core.config import load_resolved_config
from vsplit.core.index import load_index
from vsplit.schemas.manifest import STAGE_NAMES

router = APIRouter(prefix="/api/workspaces", tags=["actions"])


class RunBody(BaseModel):
    only: str | None = None
    from_: str | None = Field(default=None, alias="from")
    through: str | None = None
    concurrency_overrides: dict[str, int] | None = None

    model_config = {"populate_by_name": True}


class ResetBody(BaseModel):
    from_: str = Field(alias="from")

    model_config = {"populate_by_name": True}


def _require_slug(working_root: Path, slug: str) -> None:
    idx = load_index(working_root)
    if slug not in idx.workspaces:
        raise HTTPException(status_code=404, detail=f"workspace not found: {slug}")


@router.post("/{slug}/clean")
async def clean_one(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    n = await run_in_threadpool(clean_workspaces, root, [slug])
    return {"cleaned": n}


@router.post("/{slug}/approve")
async def approve_one(slug: str, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    n = await run_in_threadpool(approve_workspaces, root, [slug])
    return {"approved": n}


@router.post("/{slug}/reset")
async def reset_one(slug: str, body: ResetBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    if body.from_ not in STAGE_NAMES:
        raise HTTPException(status_code=422, detail=f"unknown stage: {body.from_!r}")
    await run_in_threadpool(reset_workspaces, root, [slug], from_=body.from_)
    return {"ok": True}


@router.post("/{slug}/run")
async def run_one(slug: str, body: RunBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    _require_slug(root, slug)
    cfg = load_resolved_config(working_root=root, explicit_path=None)
    if body.concurrency_overrides:
        for k, v in body.concurrency_overrides.items():
            cfg.concurrency[k] = v
    cancel_event = threading.Event()
    await run_in_threadpool(
        run_workspaces, root, [slug],
        config=cfg, cancel_event=cancel_event,
        only=body.only, from_=body.from_, through=body.through,
    )
    return {"ok": True}


batch_router = APIRouter(prefix="/api/batch", tags=["batch"])


class BatchSlugs(BaseModel):
    slugs: list[str]


class BatchReset(BatchSlugs):
    from_: str = Field(alias="from")

    model_config = {"populate_by_name": True}


class BatchRun(BatchSlugs):
    only: str | None = None
    from_: str | None = Field(default=None, alias="from")
    through: str | None = None
    concurrency_overrides: dict[str, int] | None = None

    model_config = {"populate_by_name": True}


@batch_router.post("/clean")
async def batch_clean(body: BatchSlugs, request: Request) -> dict:
    root: Path = request.app.state.working_root
    n = await run_in_threadpool(clean_workspaces, root, body.slugs)
    return {"cleaned": n}


@batch_router.post("/approve")
async def batch_approve(body: BatchSlugs, request: Request) -> dict:
    root: Path = request.app.state.working_root
    n = await run_in_threadpool(approve_workspaces, root, body.slugs)
    return {"approved": n}


@batch_router.post("/reset")
async def batch_reset(body: BatchReset, request: Request) -> dict:
    root: Path = request.app.state.working_root
    if body.from_ not in STAGE_NAMES:
        raise HTTPException(status_code=422, detail=f"unknown stage: {body.from_!r}")
    await run_in_threadpool(reset_workspaces, root, body.slugs, from_=body.from_)
    return {"ok": True}


@batch_router.post("/run")
async def batch_run(body: BatchRun, request: Request) -> dict:
    root: Path = request.app.state.working_root
    cfg = load_resolved_config(working_root=root, explicit_path=None)
    if body.concurrency_overrides:
        for k, v in body.concurrency_overrides.items():
            cfg.concurrency[k] = v
    cancel_event = threading.Event()
    await run_in_threadpool(
        run_workspaces, root, body.slugs,
        config=cfg, cancel_event=cancel_event,
        only=body.only, from_=body.from_, through=body.through,
    )
    return {"ok": True}
