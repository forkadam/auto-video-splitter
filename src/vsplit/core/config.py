from __future__ import annotations
from pathlib import Path
import yaml
from vsplit.schemas.config import Config


def _read_yaml(path: Path) -> dict:
    raw = path.read_text(encoding="utf-8")
    data = yaml.safe_load(raw) or {}
    if not isinstance(data, dict):
        raise ValueError(f"config at {path} must be a YAML mapping")
    return data


def _deep_merge(base: dict, overlay: dict) -> dict:
    out = dict(base)
    for k, v in overlay.items():
        if isinstance(v, dict) and isinstance(out.get(k), dict):
            out[k] = _deep_merge(out[k], v)
        else:
            out[k] = v
    return out


def load_resolved_config(
    *, working_root: Path, explicit_path: Path | None
) -> Config:
    """Resolution order: defaults -> .vsplit/config.yaml -> explicit_path."""
    layers: list[dict] = [{}]   # defaults handled by pydantic
    candidate = working_root / ".vsplit" / "config.yaml"
    if candidate.exists():
        layers.append(_read_yaml(candidate))
    if explicit_path is not None and explicit_path.exists():
        layers.append(_read_yaml(explicit_path))
    merged: dict = {}
    for layer in layers:
        merged = _deep_merge(merged, layer)
    return Config.model_validate(merged)
