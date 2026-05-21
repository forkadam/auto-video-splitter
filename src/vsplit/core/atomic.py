from __future__ import annotations
import json
import os
import tempfile
from pathlib import Path
from typing import Any


def atomic_write_text(path: Path, content: str, *, encoding: str = "utf-8") -> None:
    """Write-then-rename. Survives crashes mid-write. Always LF line endings."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding=encoding, newline="\n") as f:
            f.write(content)
        os.replace(tmp, path)
    except BaseException:  # includes KeyboardInterrupt / SystemExit
        try:
            os.unlink(tmp)
        except FileNotFoundError:
            pass
        raise


def atomic_write_json(path: Path, data: Any, *, indent: int = 2) -> None:
    """Serialize *data* to JSON and atomically write to *path*. Trailing newline included."""
    atomic_write_text(path, json.dumps(data, indent=indent, default=str) + "\n")
