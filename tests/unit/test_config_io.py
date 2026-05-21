from pathlib import Path
import pytest
import platformdirs as pd
from vsplit.core.config import load_resolved_config


@pytest.fixture(autouse=True)
def _isolate_user_config(tmp_path, monkeypatch):
    # Always isolate the global config dir to tmp_path so tests are deterministic
    isolated = tmp_path / "_isolated_user_config"
    monkeypatch.setattr(pd, "user_config_dir", lambda name: str(isolated / name))


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


def test_global_config_layer(tmp_path, monkeypatch):
    # Stub platformdirs.user_config_dir to point into tmp_path
    global_root = tmp_path / "user_config" / "vsplit"
    global_root.mkdir(parents=True)
    (global_root / "config.yaml").write_text("detect:\n  threshold: 11.0\n")
    monkeypatch.setattr(pd, "user_config_dir", lambda name: str(global_root.parent / name))
    working = tmp_path / "wd"
    working.mkdir()
    c = load_resolved_config(working_root=working, explicit_path=None)
    assert c.detect.threshold == 11.0


def test_working_root_overrides_global(tmp_path, monkeypatch):
    # Global says threshold=11, working_root says threshold=22 — working_root wins.
    global_root = tmp_path / "user_config" / "vsplit"
    global_root.mkdir(parents=True)
    (global_root / "config.yaml").write_text("detect:\n  threshold: 11.0\n")
    monkeypatch.setattr(pd, "user_config_dir", lambda name: str(global_root.parent / name))
    working = tmp_path / "wd"
    (working / ".vsplit").mkdir(parents=True)
    (working / ".vsplit" / "config.yaml").write_text("detect:\n  threshold: 22.0\n")
    c = load_resolved_config(working_root=working, explicit_path=None)
    assert c.detect.threshold == 22.0


def test_explicit_path_missing_raises(tmp_path):
    with pytest.raises(FileNotFoundError, match="--config path not found"):
        load_resolved_config(working_root=tmp_path, explicit_path=tmp_path / "nope.yaml")
