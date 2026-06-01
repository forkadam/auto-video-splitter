from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.actions import clean_workspaces
from vsplit.core.index import save_index, load_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> None:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc))
    save_index(tmp_path, idx)


def test_clean_workspaces_removes_dir_and_retires_slug(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")
    _seed(tmp_path, "beta")
    count = clean_workspaces(tmp_path, ["alpha"])
    assert count == 1
    assert not workspace_dir(tmp_path, "alpha").exists()
    idx = load_index(tmp_path)
    assert "alpha" not in idx.workspaces
    assert "alpha" in idx.retired_slugs
    assert "beta" in idx.workspaces


def test_clean_workspaces_silent_on_missing_slug(tmp_path: Path) -> None:
    count = clean_workspaces(tmp_path, ["does-not-exist"])
    assert count == 0


def test_clean_keeps_entry_when_removal_fails(tmp_path: Path, monkeypatch) -> None:
    """If a workspace dir can't be deleted (held open), keep its index entry —
    don't silently orphan the folder by popping the row while files remain."""
    import vsplit.core.actions as actions
    _seed(tmp_path, "alpha")
    _seed(tmp_path, "beta")
    real = actions._robust_rmtree

    def fake(path: Path) -> bool:
        if path.name == "alpha":
            return False  # simulate a stuck lock
        return real(path)

    monkeypatch.setattr(actions, "_robust_rmtree", fake)
    count = clean_workspaces(tmp_path, ["alpha", "beta"])
    assert count == 1  # only beta removed
    idx = load_index(tmp_path)
    assert "alpha" in idx.workspaces           # kept, not orphaned
    assert "alpha" not in idx.retired_slugs
    assert "beta" not in idx.workspaces
    assert "beta" in idx.retired_slugs


def test_robust_rmtree_retries_transient_lock(tmp_path: Path, monkeypatch) -> None:
    import shutil
    import vsplit.core.actions as actions
    d = tmp_path / "d" / "sub"
    d.mkdir(parents=True)
    (d / "f.txt").write_text("x")
    calls = {"n": 0}
    real_rmtree = shutil.rmtree

    def flaky(path, *a, **k):
        calls["n"] += 1
        if calls["n"] == 1:
            raise PermissionError("locked")
        return real_rmtree(path, *a, **k)

    monkeypatch.setattr(actions.shutil, "rmtree", flaky)
    monkeypatch.setattr(actions.time, "sleep", lambda *_a: None)
    assert actions._robust_rmtree(tmp_path / "d") is True
    assert calls["n"] == 2
    assert not (tmp_path / "d").exists()
