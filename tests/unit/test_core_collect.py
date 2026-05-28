"""Unit tests for vsplit.core.collect — gathering each workspace's `output/`
clips into a single destination directory.

The seeding helper mirrors tests/unit/test_dashboard_api_workspaces.py so we
get a registered slug + a manifest + a fake output/ dir with clip files.
"""
from __future__ import annotations
from datetime import datetime, timezone
from pathlib import Path

import pytest

from vsplit.core.collect import collect_outputs
from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import load_manifest, new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str, *, clip_names: list[str], split_done: bool = True) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    if split_done:
        m.stages["split"].status = "done"
        m.stages["split"].artifacts["outputs"] = clip_names
    save_manifest(ws, m)

    out = ws / "output"
    out.mkdir(parents=True)
    for name in clip_names:
        (out / name).write_bytes(f"{slug}/{name}".encode())

    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)


def test_collect_copy_default_destination(tmp_path: Path) -> None:
    """No slugs → collects every split=done workspace into <root>/collected/.
    Mode defaults to copy, so the originals stay put."""
    _seed(tmp_path, "alpha", clip_names=["alpha-highlight-01.mp4"])
    _seed(tmp_path, "beta", clip_names=["beta-highlight-01.mp4", "beta-highlight-02.mp4"])

    result = collect_outputs(tmp_path)

    dest = tmp_path / "collected"
    assert dest.is_dir()
    assert (dest / "alpha-highlight-01.mp4").read_bytes() == b"alpha/alpha-highlight-01.mp4"
    assert (dest / "beta-highlight-01.mp4").read_bytes() == b"beta/beta-highlight-01.mp4"
    assert (dest / "beta-highlight-02.mp4").read_bytes() == b"beta/beta-highlight-02.mp4"
    # Originals untouched (copy mode).
    assert (workspace_dir(tmp_path, "alpha") / "output" / "alpha-highlight-01.mp4").exists()
    assert result["collected"] == 3
    assert result["from_workspaces"] == 2
    assert result["skipped_not_done"] == 0
    assert result["skipped_existing"] == 0


def test_collect_skips_workspaces_with_split_not_done(tmp_path: Path) -> None:
    """A workspace whose split stage hasn't finished must be skipped silently —
    you collect when ready, not in the middle of a run."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"], split_done=True)
    _seed(tmp_path, "beta", clip_names=["beta-01.mp4"], split_done=False)

    result = collect_outputs(tmp_path)
    dest = tmp_path / "collected"
    assert (dest / "alpha-01.mp4").exists()
    assert not (dest / "beta-01.mp4").exists()
    assert result["collected"] == 1
    assert result["skipped_not_done"] == 1


def test_collect_explicit_slug_filter(tmp_path: Path) -> None:
    """Passing slugs limits collection to that subset."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    _seed(tmp_path, "beta", clip_names=["beta-01.mp4"])
    _seed(tmp_path, "gamma", clip_names=["gamma-01.mp4"])

    result = collect_outputs(tmp_path, slugs=["alpha", "gamma"])
    dest = tmp_path / "collected"
    assert (dest / "alpha-01.mp4").exists()
    assert not (dest / "beta-01.mp4").exists()
    assert (dest / "gamma-01.mp4").exists()
    assert result["collected"] == 2


def test_collect_existing_file_is_skipped_without_force(tmp_path: Path) -> None:
    """Skip-existing is the safe default — don't silently overwrite a file
    that already happens to be at the destination."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    dest = tmp_path / "collected"
    dest.mkdir()
    (dest / "alpha-01.mp4").write_bytes(b"existing content")

    result = collect_outputs(tmp_path)
    assert (dest / "alpha-01.mp4").read_bytes() == b"existing content"
    assert result["collected"] == 0
    assert result["skipped_existing"] == 1


def test_collect_force_overwrites_existing(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    dest = tmp_path / "collected"
    dest.mkdir()
    (dest / "alpha-01.mp4").write_bytes(b"existing content")

    result = collect_outputs(tmp_path, force=True)
    assert (dest / "alpha-01.mp4").read_bytes() == b"alpha/alpha-01.mp4"
    assert result["collected"] == 1
    assert result["skipped_existing"] == 0


def test_collect_move_mode_removes_source(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    src_path = workspace_dir(tmp_path, "alpha") / "output" / "alpha-01.mp4"
    assert src_path.exists()

    result = collect_outputs(tmp_path, mode="move")
    dest = tmp_path / "collected"
    assert (dest / "alpha-01.mp4").exists()
    assert not src_path.exists(), "move mode must remove the source file"
    assert result["collected"] == 1


def test_collect_custom_destination(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    dest = tmp_path / "elsewhere" / "final"

    collect_outputs(tmp_path, dest=dest)
    assert (dest / "alpha-01.mp4").exists()


def test_collect_workspace_with_no_output_dir_is_skipped(tmp_path: Path) -> None:
    """split.status == done but the output dir got removed somehow — don't crash."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    out = workspace_dir(tmp_path, "alpha") / "output"
    for f in out.iterdir():
        f.unlink()
    out.rmdir()

    result = collect_outputs(tmp_path)
    assert result["collected"] == 0
    assert result["skipped_no_output"] == 1


def test_collect_unknown_mode_raises(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    with pytest.raises(ValueError, match="mode"):
        collect_outputs(tmp_path, mode="banana")  # type: ignore[arg-type]


def test_collect_dry_run_reports_but_writes_nothing(tmp_path: Path) -> None:
    """Preview mode: counts what would happen so the dashboard can render a
    confirm dialog. Must not touch the filesystem at the destination."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    _seed(tmp_path, "beta", clip_names=["beta-01.mp4", "beta-02.mp4"])
    _seed(tmp_path, "gamma", clip_names=["gamma-01.mp4"], split_done=False)
    dest = tmp_path / "preview-dest"
    (dest).mkdir()
    (dest / "beta-02.mp4").write_bytes(b"already here")

    result = collect_outputs(tmp_path, dest=dest, dry_run=True)

    # Counts reflect what *would* happen.
    assert result["collected"] == 2  # alpha-01 + beta-01
    assert result["from_workspaces"] == 2
    assert result["skipped_existing"] == 1  # beta-02 already at dest
    assert result["skipped_not_done"] == 1  # gamma

    # And nothing was actually written.
    assert sorted(p.name for p in dest.iterdir()) == ["beta-02.mp4"]
    assert (dest / "beta-02.mp4").read_bytes() == b"already here"
    # Originals untouched too — dry-run never moves either.
    assert (workspace_dir(tmp_path, "alpha") / "output" / "alpha-01.mp4").exists()


def test_collect_dry_run_with_move_mode_does_not_remove_sources(tmp_path: Path) -> None:
    """Even when the caller specified mode=move, dry-run must keep sources."""
    _seed(tmp_path, "alpha", clip_names=["alpha-01.mp4"])
    src = workspace_dir(tmp_path, "alpha") / "output" / "alpha-01.mp4"

    result = collect_outputs(tmp_path, mode="move", dry_run=True)
    assert result["collected"] == 1
    assert src.exists(), "dry-run must not delete source files even in move mode"
