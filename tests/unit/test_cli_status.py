from pathlib import Path
from click.testing import CliRunner
from vsplit.cli import main

def _make_video(p: Path) -> None:
    p.write_bytes(b"\x00" * 2_000_000)

def test_status_empty(tmp_path):
    runner = CliRunner()
    result = runner.invoke(main, ["-w", str(tmp_path), "status"])
    assert result.exit_code == 0
    assert "0 workspaces" in result.output

def test_status_after_scan(tmp_path):
    _make_video(tmp_path / "Birthday.mov")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    result = runner.invoke(main, ["-w", str(tmp_path), "status"])
    assert result.exit_code == 0
    assert "birthday" in result.output

def test_status_json(tmp_path):
    _make_video(tmp_path / "X.mp4")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    result = runner.invoke(main, ["-w", str(tmp_path), "--json", "status"])  # --json BEFORE status
    assert result.exit_code == 0
    import json
    data = json.loads(result.output)
    assert "workspaces" in data
