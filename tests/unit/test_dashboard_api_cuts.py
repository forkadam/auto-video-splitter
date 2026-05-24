import json
from datetime import datetime, timezone
from pathlib import Path

from fastapi.testclient import TestClient

from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.server import create_app
from vsplit.schemas.index import WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str) -> Path:
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    idx = load_index(tmp_path)
    idx.workspaces[slug] = WorkspaceEntry(
        source_relative_path=f"{slug}.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )
    save_index(tmp_path, idx)
    return ws


def _seed_cuts(ws: Path, *, scene_ids: list[int]) -> Path:
    """Write detect/scenes.json + decisions/cuts.json with matching IDs."""
    (ws / "detect").mkdir(parents=True, exist_ok=True)
    (ws / "detect" / "scenes.json").write_text(json.dumps({
        "schema_version": 1,
        "scenes": [{"id": i, "start_time": float(i), "end_time": float(i) + 1.0}
                   for i in scene_ids],
    }))
    (ws / "decisions").mkdir(parents=True, exist_ok=True)
    cuts = {
        "schema_version": 1,
        "scenes": [
            {"id": i, "start": f"00:00:0{i}.000", "end": f"00:00:0{i+1}.000",
             "keep": True, "label": None, "merge_with_next": False}
            for i in scene_ids
        ],
    }
    (ws / "decisions" / "cuts.json").write_text(json.dumps(cuts, indent=2) + "\n")
    return ws / "decisions" / "cuts.json"


def test_get_cuts_happy_path(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/cuts")
    assert r.status_code == 200
    body = r.json()
    assert body["cuts"]["schema_version"] == 1
    assert len(body["cuts"]["scenes"]) == 2
    assert body["cuts"]["scenes"][0]["id"] == 1
    assert isinstance(body["hash"], str) and body["hash"].startswith("sha256:")
    assert isinstance(body["scene_thumbs"], dict)


def test_get_cuts_404_unknown_slug(tmp_path: Path) -> None:
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/nope/cuts")
    assert r.status_code == 404


def test_get_cuts_404_when_review_not_run(tmp_path: Path) -> None:
    _seed(tmp_path, "alpha")  # no cuts.json
    client = TestClient(create_app(tmp_path))
    r = client.get("/api/workspaces/alpha/cuts")
    assert r.status_code == 404


def test_get_cuts_scene_thumbs_first_match(tmp_path: Path) -> None:
    ws = _seed(tmp_path, "alpha")
    _seed_cuts(ws, scene_ids=[1, 2])
    img_dir = ws / "preview" / "images"
    img_dir.mkdir(parents=True)
    # Multiple thumbs per scene; expect first sorted match per id.
    (img_dir / "scene-001-00.jpg").write_bytes(b"a")
    (img_dir / "scene-001-01.jpg").write_bytes(b"b")
    (img_dir / "scene-002-00.jpg").write_bytes(b"c")
    # Decoy: scene-003 has no cuts entry.
    (img_dir / "scene-003-00.jpg").write_bytes(b"d")
    client = TestClient(create_app(tmp_path))
    body = client.get("/api/workspaces/alpha/cuts").json()
    assert body["scene_thumbs"] == {"1": "scene-001-00.jpg", "2": "scene-002-00.jpg"}
