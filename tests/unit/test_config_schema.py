from vsplit.schemas.config import Config

def test_config_defaults():
    c = Config()
    assert c.detect.threshold == 27.0
    assert c.detect.detector == "content"
    assert c.review.default_keep is True
    assert c.split.output_container == "mp4"
    assert c.concurrency["split"] == 1
    assert "mp4" in c.scan.extensions

def test_config_partial_override():
    c = Config.model_validate({"review": {"default_keep": False}})
    assert c.review.default_keep is False
    assert c.detect.threshold == 27.0   # untouched


def test_config_concurrency_rejects_unknown_stage():
    import pytest
    from pydantic import ValidationError
    with pytest.raises(ValidationError, match="unknown stage"):
        Config.model_validate({"concurrency": {"typo_stage": 4}})


def test_config_concurrency_rejects_non_positive():
    import pytest
    from pydantic import ValidationError
    with pytest.raises(ValidationError, match=">= 1"):
        Config.model_validate({"concurrency": {"probe": 0}})
