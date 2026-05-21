import json
from pathlib import Path

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
