from datetime import datetime, timezone
from vsplit.schemas.index import Index, WorkspaceEntry

def test_index_round_trip():
    data = {
        "schema_version": 1,
        "working_root": ".",
        "workspaces": {
            "birthday": {
                "source_relative_path": "Birthday.mov",
                "size_bytes": 100,
                "partial_hash": "sha256:x",
                "adopted_at": datetime.now(timezone.utc).isoformat(),
            }
        },
        "retired_slugs": ["old-slug"],
    }
    idx = Index.model_validate(data)
    assert "birthday" in idx.workspaces
    assert idx.retired_slugs == ["old-slug"]

def test_index_defaults_for_missing_optional_fields():
    idx = Index(schema_version=1, working_root=".", workspaces={})
    assert idx.retired_slugs == []
