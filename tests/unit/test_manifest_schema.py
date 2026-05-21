from datetime import datetime, timezone
import pytest
from pydantic import ValidationError
from vsplit.schemas.manifest import Manifest, StageBlock, SourceInfo

def _now() -> str:
    return datetime.now(timezone.utc).isoformat()

def test_manifest_minimal_round_trip():
    data = {
        "schema_version": 1,
        "id": "birthday",
        "created_at": _now(),
        "updated_at": _now(),
        "source": {
            "relative_path": "Birthday.mov",
            "size_bytes": 100,
            "mtime": _now(),
            "partial_hash": "sha256:abc",
        },
        "stages": {
            name: {"status": "pending", "artifacts": {}}
            for name in ["probe", "detect", "preview", "review", "plan", "split", "verify"]
        },
    }
    m = Manifest.model_validate(data)
    assert m.id == "birthday"
    assert m.stages["probe"].status == "pending"

def test_manifest_rejects_unknown_stage_status():
    data = {
        "schema_version": 1,
        "id": "x",
        "created_at": _now(),
        "updated_at": _now(),
        "source": {"relative_path": "x", "size_bytes": 1, "mtime": _now(), "partial_hash": "x"},
        "stages": {
            "probe": {"status": "BOGUS", "artifacts": {}},
            "detect": {"status": "pending", "artifacts": {}},
            "preview": {"status": "pending", "artifacts": {}},
            "review": {"status": "pending", "artifacts": {}},
            "plan": {"status": "pending", "artifacts": {}},
            "split": {"status": "pending", "artifacts": {}},
            "verify": {"status": "pending", "artifacts": {}},
        },
    }
    with pytest.raises(ValidationError):
        Manifest.model_validate(data)
