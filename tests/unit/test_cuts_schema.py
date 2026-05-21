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
