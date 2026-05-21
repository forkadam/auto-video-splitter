import pytest
import platformdirs as _pd


@pytest.fixture(autouse=True)
def _isolate_user_config(tmp_path_factory, monkeypatch):
    isolated = tmp_path_factory.mktemp("user_config_isolated")
    monkeypatch.setattr(_pd, "user_config_dir", lambda name: str(isolated / name))
