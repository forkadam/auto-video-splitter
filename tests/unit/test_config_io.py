from pathlib import Path
from vsplit.core.config import load_resolved_config

def test_no_file_returns_defaults(tmp_path):
    c = load_resolved_config(working_root=tmp_path, explicit_path=None)
    assert c.review.default_keep is True
    assert c.detect.threshold == 27.0

def test_working_root_yaml_overrides_defaults(tmp_path):
    (tmp_path / ".vsplit").mkdir()
    (tmp_path / ".vsplit" / "config.yaml").write_text(
        "review:\n  default_keep: false\n"
    )
    c = load_resolved_config(working_root=tmp_path, explicit_path=None)
    assert c.review.default_keep is False

def test_explicit_path_wins(tmp_path):
    p = tmp_path / "custom.yaml"
    p.write_text("detect:\n  threshold: 42.0\n")
    c = load_resolved_config(working_root=tmp_path, explicit_path=p)
    assert c.detect.threshold == 42.0
