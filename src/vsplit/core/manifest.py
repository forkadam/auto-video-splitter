from __future__ import annotations
import json
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.atomic import atomic_write_text
from vsplit.schemas.manifest import (
    Manifest, SourceInfo, StageBlock, STAGE_NAMES
)

MANIFEST_FILENAME = "manifest.json"


def manifest_path(workspace_dir: Path) -> Path:
    return workspace_dir / MANIFEST_FILENAME


def new_manifest(*, slug: str, source: SourceInfo) -> Manifest:
    now = datetime.now(timezone.utc)
    return Manifest(
        id=slug,
        created_at=now,
        updated_at=now,
        source=source,
        stages={name: StageBlock(status="pending") for name in STAGE_NAMES},
    )


def load_manifest(workspace_dir: Path) -> Manifest:
    p = manifest_path(workspace_dir)
    if not p.exists():
        raise FileNotFoundError(f"manifest not found at {p}")
    data = json.loads(p.read_text(encoding="utf-8"))
    return Manifest.model_validate(data)


def save_manifest(workspace_dir: Path, manifest: Manifest) -> None:
    manifest.updated_at = datetime.now(timezone.utc)
    payload = manifest.model_dump(mode="json")
    atomic_write_text(
        manifest_path(workspace_dir),
        json.dumps(payload, indent=2, default=str) + "\n",
    )
