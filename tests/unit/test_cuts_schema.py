import pytest
from pydantic import ValidationError
from vsplit.schemas.cuts import CutsJson, Scene

def test_cuts_minimal():
    data = {
        "schema_version": 1,
        "scenes": [
            {"id": 1, "start": "00:00:00.000", "end": "00:00:10.000", "keep": True},
        ],
    }
    c = CutsJson.model_validate(data)
    assert c.scenes[0].id == 1
    assert c.scenes[0].keep is True
    assert c.scenes[0].label is None
    assert c.scenes[0].merge_with_next is False

def test_cuts_rejects_unknown_field():
    data = {
        "schema_version": 1,
        "scenes": [
            {"id": 1, "start": "00:00:00.000", "end": "00:00:10.000", "keep": True, "bogus": 1},
        ],
    }
    with pytest.raises(ValidationError):
        CutsJson.model_validate(data)


def test_cuts_rejects_bad_timestamp():
    bad = {
        "schema_version": 1,
        "scenes": [{"id": 1, "start": "1:2:3", "end": "00:00:01.000", "keep": True}],
    }
    with pytest.raises(ValidationError, match="HH:MM:SS"):
        CutsJson.model_validate(bad)


def test_cuts_rejects_duplicate_ids():
    bad = {
        "schema_version": 1,
        "scenes": [
            {"id": 1, "start": "00:00:00.000", "end": "00:00:01.000", "keep": True},
            {"id": 1, "start": "00:00:01.000", "end": "00:00:02.000", "keep": True},
        ],
    }
    with pytest.raises(ValidationError, match="duplicate"):
        CutsJson.model_validate(bad)
