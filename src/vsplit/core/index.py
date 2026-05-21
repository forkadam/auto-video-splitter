from __future__ import annotations
import json
from pathlib import Path
from vsplit.core.atomic import atomic_write_text
from vsplit.core.paths import working_state_dir
from vsplit.schemas.index import Index

INDEX_FILENAME = "index.json"


def index_path(working_root: Path) -> Path:
    return working_state_dir(working_root) / INDEX_FILENAME


def load_index(working_root: Path) -> Index:
    p = index_path(working_root)
    if not p.exists():
        return Index()
    data = json.loads(p.read_text(encoding="utf-8"))
    return Index.model_validate(data)


def save_index(working_root: Path, idx: Index) -> None:
    payload = idx.model_dump(mode="json")
    atomic_write_text(
        index_path(working_root),
        json.dumps(payload, indent=2, default=str) + "\n",
    )
