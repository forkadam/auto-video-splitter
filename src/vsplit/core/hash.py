from __future__ import annotations
import hashlib
from pathlib import Path

_CHUNK = 1024 * 1024  # 1 MB


def partial_hash(path: Path) -> str:
    """sha256(first 1MB + last 1MB + size_bytes_big_endian_8). Fast identity."""
    size = path.stat().st_size
    h = hashlib.sha256()
    with path.open("rb") as f:
        if size <= 2 * _CHUNK:
            h.update(f.read())
        else:
            h.update(f.read(_CHUNK))
            f.seek(-_CHUNK, 2)
            h.update(f.read(_CHUNK))
    h.update(size.to_bytes(8, "big"))
    return f"sha256:{h.hexdigest()}"


def content_hash(path: Path) -> str:
    """Full sha256 of file contents. Used for cuts.json change detection."""
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(_CHUNK), b""):
            h.update(chunk)
    return f"sha256:{h.hexdigest()}"
