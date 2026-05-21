import pytest
from vsplit.cli._filter import parse_filter, apply_filters, Predicate

def test_parse_key_value():
    p = parse_filter("status=failed")
    assert p == Predicate(key="status", value="failed")

def test_parse_bad():
    with pytest.raises(ValueError):
        parse_filter("statusfailed")

def test_apply_status():
    workspaces = {
        "a": {"any_status": ["done"]},
        "b": {"any_status": ["failed"]},
        "c": {"any_status": ["awaiting_user"]},
    }
    def has_status(slug, val):
        return val in workspaces[slug]["any_status"]
    preds = [parse_filter("status=failed")]
    result = apply_filters(list(workspaces), preds,
                           status_pred=has_status, stage_pred=lambda *a: True)
    assert result == ["b"]

def test_apply_slug_glob():
    preds = [parse_filter("slug=vacation-*")]
    result = apply_filters(
        ["vacation-1", "vacation-2", "other"], preds,
        status_pred=lambda *a: True, stage_pred=lambda *a: True,
    )
    assert sorted(result) == ["vacation-1", "vacation-2"]
