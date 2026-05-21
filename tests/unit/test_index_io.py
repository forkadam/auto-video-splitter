from datetime import datetime, timezone
from vsplit.core.index import load_index, save_index, index_path
from vsplit.schemas.index import Index, WorkspaceEntry

def test_load_returns_empty_for_missing(tmp_path):
    idx = load_index(tmp_path)
    assert idx.workspaces == {}
    assert idx.retired_slugs == []

def test_round_trip(tmp_path):
    idx = Index(
        workspaces={
            "x": WorkspaceEntry(
                source_relative_path="X.mov",
                size_bytes=10,
                partial_hash="sha256:x",
                adopted_at=datetime.now(timezone.utc),
            )
        },
        retired_slugs=["old"],
    )
    save_index(tmp_path, idx)
    assert index_path(tmp_path).exists()
    loaded = load_index(tmp_path)
    assert "x" in loaded.workspaces
    assert loaded.retired_slugs == ["old"]
