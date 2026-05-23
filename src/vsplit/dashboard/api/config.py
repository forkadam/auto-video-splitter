from __future__ import annotations
from pathlib import Path

import yaml
from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, ValidationError

from vsplit.core.atomic import atomic_write_text
from vsplit.core.config import load_resolved_config
from vsplit.core.paths import working_state_dir
from vsplit.schemas.config import Config

router = APIRouter(prefix="/api/config", tags=["config"])


@router.get("")
def get_config(request: Request) -> dict:
    root: Path = request.app.state.working_root
    cfg = load_resolved_config(working_root=root, explicit_path=None)
    return cfg.model_dump(mode="json")


@router.get("/raw")
def get_raw(request: Request) -> dict:
    root: Path = request.app.state.working_root
    p = root / ".vsplit" / "config.yaml"
    text = p.read_text(encoding="utf-8") if p.is_file() else ""
    return {"text": text}


@router.get("/schema")
def get_schema() -> dict:
    return Config.model_json_schema()


class RawBody(BaseModel):
    text: str


def _config_path(root: Path) -> Path:
    return root / ".vsplit" / "config.yaml"


@router.put("")
def put_config(payload: dict, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        Config.model_validate(payload)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors())
    text = yaml.safe_dump(payload, sort_keys=False)
    working_state_dir(root).mkdir(parents=True, exist_ok=True)
    atomic_write_text(_config_path(root), text)
    return {"ok": True}


@router.put("/raw")
def put_raw(body: RawBody, request: Request) -> dict:
    root: Path = request.app.state.working_root
    try:
        parsed = yaml.safe_load(body.text) or {}
    except yaml.YAMLError as e:
        mark = getattr(e, "problem_mark", None)
        raise HTTPException(status_code=400, detail={
            "line": mark.line + 1 if mark else None,
            "col": mark.column + 1 if mark else None,
            "message": str(e),
        })
    if not isinstance(parsed, dict):
        raise HTTPException(status_code=400, detail="YAML root must be a mapping")
    try:
        Config.model_validate(parsed)
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors())
    working_state_dir(root).mkdir(parents=True, exist_ok=True)
    atomic_write_text(_config_path(root), body.text)
    return {"ok": True}
