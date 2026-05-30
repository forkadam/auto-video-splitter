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

def test_manifest_rejects_missing_stage_key():
    from datetime import datetime, timezone
    bad = {
        "schema_version": 1,
        "id": "x",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "updated_at": datetime.now(timezone.utc).isoformat(),
        "source": {"relative_path": "x", "size_bytes": 1, "mtime": datetime.now(timezone.utc).isoformat(), "partial_hash": "x"},
        "stages": {n: {"status": "pending", "artifacts": {}} for n in ["probe", "detect"]},  # missing the other 5
    }
    with pytest.raises(ValidationError):
        Manifest.model_validate(bad)


def test_manifest_rejects_unknown_stage_key():
    from datetime import datetime, timezone
    full_stages = {n: {"status": "pending", "artifacts": {}}
                   for n in ["probe", "detect", "preview", "review", "plan", "split", "verify"]}
    full_stages["bogus_extra"] = {"status": "pending", "artifacts": {}}
    bad = {
        "schema_version": 1,
        "id": "x",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "updated_at": datetime.now(timezone.utc).isoformat(),
        "source": {"relative_path": "x", "size_bytes": 1, "mtime": datetime.now(timezone.utc).isoformat(), "partial_hash": "x"},
        "stages": full_stages,
    }
    with pytest.raises(ValidationError):
        Manifest.model_validate(bad)


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


def test_manifest_ignored_defaults_false_when_absent():
    """Existing manifests (no `ignored` key) load with ignored=False, ignored_at=None."""
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
    assert m.ignored is False
    assert m.ignored_at is None


def test_manifest_ignored_round_trips():
    data = {
        "schema_version": 1,
        "id": "birthday",
        "created_at": _now(),
        "updated_at": _now(),
        "ignored": True,
        "ignored_at": _now(),
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
    assert m.ignored is True
    assert m.ignored_at is not None
    m2 = Manifest.model_validate(m.model_dump(mode="json"))
    assert m2.ignored is True
