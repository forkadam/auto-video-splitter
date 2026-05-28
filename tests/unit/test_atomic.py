import json
import os
from pathlib import Path

import pytest

from vsplit.core.atomic import atomic_write_text, atomic_write_json

def test_atomic_write_text(tmp_path: Path):
    p = tmp_path / "out.txt"
    atomic_write_text(p, "hello\n")
    assert p.read_text() == "hello\n"

def test_atomic_write_overwrites(tmp_path: Path):
    p = tmp_path / "out.txt"
    p.write_text("old")
    atomic_write_text(p, "new")
    assert p.read_text() == "new"

def test_atomic_write_json(tmp_path: Path):
    p = tmp_path / "out.json"
    atomic_write_json(p, {"a": 1, "b": [2, 3]})
    assert json.loads(p.read_text()) == {"a": 1, "b": [2, 3]}

def test_atomic_write_creates_parent(tmp_path: Path):
    p = tmp_path / "sub" / "out.txt"
    atomic_write_text(p, "x")
    assert p.read_text() == "x"


def test_atomic_write_retries_transient_permission_error(tmp_path: Path, monkeypatch):
    """On Windows, os.replace can transiently fail with PermissionError when the
    destination is held open by a concurrent reader (e.g., the dashboard watcher
    polling manifest.json every 0.5s). The write must retry and succeed."""
    p = tmp_path / "out.txt"
    p.write_text("old")

    real_replace = os.replace
    calls = {"n": 0}

    def flaky_replace(src, dst):
        calls["n"] += 1
        if calls["n"] < 3:
            raise PermissionError(13, "The process cannot access the file because it is being used by another process")
        return real_replace(src, dst)

    monkeypatch.setattr("vsplit.core.atomic.os.replace", flaky_replace)
    # Speed up the test: shrink the initial backoff so we don't actually sleep.
    monkeypatch.setattr("vsplit.core.atomic._REPLACE_RETRY_INITIAL", 0.001)

    atomic_write_text(p, "new")
    assert p.read_text() == "new"
    assert calls["n"] == 3, "should have retried twice before succeeding"


def test_atomic_write_gives_up_after_max_retries(tmp_path: Path, monkeypatch):
    """If os.replace keeps failing, atomic_write_text must eventually raise so the
    caller (e.g., pipeline.run_stage) can react instead of silently dropping data."""
    p = tmp_path / "out.txt"
    p.write_text("old")

    def always_locked(src, dst):
        raise PermissionError(13, "locked")

    monkeypatch.setattr("vsplit.core.atomic.os.replace", always_locked)
    monkeypatch.setattr("vsplit.core.atomic._REPLACE_RETRY_INITIAL", 0.001)

    with pytest.raises(PermissionError):
        atomic_write_text(p, "new")
    # Original file unchanged; no half-written temp file left behind.
    assert p.read_text() == "old"
    leftovers = [x for x in tmp_path.iterdir() if x.name.startswith(".")]
    assert leftovers == [], f"tempfile not cleaned up: {leftovers}"
