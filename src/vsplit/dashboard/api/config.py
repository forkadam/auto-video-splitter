from __future__ import annotations
from pathlib import Path
from fastapi import APIRouter, Request

from vsplit.core.config import load_resolved_config
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
