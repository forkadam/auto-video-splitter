from pathlib import Path
from click.testing import CliRunner
from vsplit.cli import main
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest, manifest_path


def _make_video(p: Path, size: int = 2 * 1024 * 1024) -> None:
    p.write_bytes(b"\x00" * size)


def test_scan_adopts_files(tmp_path):
    _make_video(tmp_path / "Birthday.mov")
    _make_video(tmp_path / "Beach Day.mp4")
    runner = CliRunner()
    result = runner.invoke(main, ["-w", str(tmp_path), "scan"])
    assert result.exit_code == 0, result.output
    idx = load_index(tmp_path)
    assert "birthday" in idx.workspaces
    assert "beach-day" in idx.workspaces


def test_scan_creates_manifest(tmp_path):
    _make_video(tmp_path / "Hike.mkv")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    ws = tmp_path / ".vsplit" / "workspaces" / "hike"
    assert manifest_path(ws).exists()
    m = load_manifest(ws)
    assert m.source.relative_path == "Hike.mkv"


def test_scan_idempotent(tmp_path):
    _make_video(tmp_path / "x.mp4")
    runner = CliRunner()
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    runner.invoke(main, ["-w", str(tmp_path), "scan"])
    idx = load_index(tmp_path)
    assert len(idx.workspaces) == 1
