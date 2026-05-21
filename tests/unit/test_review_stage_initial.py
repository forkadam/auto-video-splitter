import json
import threading
import logging
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.manifest import new_manifest
from vsplit.core.types import StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.stages.review import ReviewStage


def _ctx(tmp_path: Path, scenes: list[dict], default_keep: bool) -> StageContext:
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "detect").mkdir()
    (ws / "detect" / "scenes.json").write_text(json.dumps({
        "detector": "adaptive", "config": {}, "scenes": scenes,
    }))
    m = new_manifest(slug="ws", source=SourceInfo(
        relative_path="x", size_bytes=1, mtime=datetime.now(timezone.utc), partial_hash="x"))
    cfg = Config()
    cfg.review.default_keep = default_keep
    return StageContext(
        workspace_dir=ws, working_root=tmp_path, source_path=ws / "x",
        manifest=m, config=cfg, logger=logging.getLogger("t"),
        cancel_event=threading.Event(),
    )


def test_review_pre_populates_and_awaits_user(tmp_path):
    ctx = _ctx(tmp_path, [
        {"id": 1, "start_time": 0.0, "end_time": 1.0, "start_frame": 0, "end_frame": 24},
        {"id": 2, "start_time": 1.0, "end_time": 2.0, "start_frame": 24, "end_frame": 48},
    ], default_keep=True)
    result = ReviewStage().run(ctx)
    assert result.status == "awaiting_user"
    data = json.loads((ctx.workspace_dir / "decisions" / "cuts.json").read_text())
    assert all(s["keep"] is True for s in data["scenes"])
    assert "cuts_file_hash" in result.artifacts
    assert result.artifacts["scenes_total"] == 2
    assert result.artifacts["scenes_marked_keep_initial"] == 2


def test_review_default_keep_false_for_curation(tmp_path):
    ctx = _ctx(tmp_path, [
        {"id": 1, "start_time": 0.0, "end_time": 1.0, "start_frame": 0, "end_frame": 24},
    ], default_keep=False)
    result = ReviewStage().run(ctx)
    data = json.loads((ctx.workspace_dir / "decisions" / "cuts.json").read_text())
    assert data["scenes"][0]["keep"] is False
    assert result.artifacts["scenes_marked_keep_initial"] == 0
