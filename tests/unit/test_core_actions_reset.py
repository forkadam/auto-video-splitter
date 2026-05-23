from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.actions import reset_workspaces
from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest, load_manifest
from vsplit.core.paths import workspace_dir
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path):
    slug = "alpha"
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    (ws / "probe").mkdir(); (ws / "probe" / "ffprobe.json").write_text("{}")
    (ws / "detect").mkdir(); (ws / "detect" / "scenes.json").write_text("{}")
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path="alpha.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    m.stages["probe"].status = "done"
    m.stages["detect"].status = "done"
    save_manifest(ws, m)
    save_index(tmp_path, Index(workspaces={slug: WorkspaceEntry(
        source_relative_path="alpha.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    return ws


def test_reset_from_detect_clears_detect_onwards(tmp_path: Path) -> None:
    ws = _seed(tmp_path)
    reset_workspaces(tmp_path, ["alpha"], from_="detect")
    m = load_manifest(ws)
    assert m.stages["probe"].status == "done"
    assert m.stages["detect"].status == "pending"
    assert m.stages["preview"].status == "pending"
    assert not (ws / "detect").exists()
