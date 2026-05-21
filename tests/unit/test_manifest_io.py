from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.manifest import (
    load_manifest, save_manifest, new_manifest, manifest_path
)
from vsplit.schemas.manifest import Manifest, SourceInfo

def test_round_trip(tmp_path):
    ws = tmp_path / "birthday"
    ws.mkdir()
    m = new_manifest(
        slug="birthday",
        source=SourceInfo(
            relative_path="Birthday.mov",
            size_bytes=100,
            mtime=datetime.now(timezone.utc),
            partial_hash="sha256:abc",
        ),
    )
    save_manifest(ws, m)
    assert manifest_path(ws).exists()
    loaded = load_manifest(ws)
    assert loaded.id == "birthday"
    assert loaded.stages["probe"].status == "pending"

def test_load_missing_raises(tmp_path):
    import pytest
    with pytest.raises(FileNotFoundError):
        load_manifest(tmp_path)
