from __future__ import annotations
import json
import os
import tempfile
import time
from pathlib import Path
from typing import Any

# On Windows, os.replace can transiently raise PermissionError if the destination
# is briefly held open by a concurrent reader (the dashboard watcher polls
# manifest.json every 0.5s). Retry with exponential backoff so legitimate writes
# don't surface as "stage stuck running" bugs upstream.
_REPLACE_RETRY_INITIAL = 0.05
_REPLACE_RETRY_MAX = 6


def _replace_with_retry(src: str, dst: Path) -> None:
    delay = _REPLACE_RETRY_INITIAL
    for attempt in range(_REPLACE_RETRY_MAX):
        try:
            os.replace(src, dst)
            return
        except PermissionError:
            if attempt == _REPLACE_RETRY_MAX - 1:
                raise
            time.sleep(delay)
            delay *= 2


def atomic_write_text(path: Path, content: str, *, encoding: str = "utf-8") -> None:
    """Write-then-rename. Survives crashes mid-write. Always LF line endings."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding=encoding, newline="\n") as f:
            f.write(content)
        _replace_with_retry(tmp, path)
    except BaseException:  # includes KeyboardInterrupt / SystemExit
        try:
            os.unlink(tmp)
        except FileNotFoundError:
            pass
        raise


def atomic_write_json(path: Path, data: Any, *, indent: int = 2) -> None:
    """Serialize *data* to JSON and atomically write to *path*. Trailing newline included."""
    atomic_write_text(path, json.dumps(data, indent=indent, default=str) + "\n")
