import json
import threading
import logging
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.manifest import new_manifest
from vsplit.core.hash import content_hash
from vsplit.core.types import StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo, StageBlock
from vsplit.stages.review import ReviewStage

def _setup(tmp_path: Path, initial_state: str = "fresh"):
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "detect").mkdir()
    (ws / "detect" / "scenes.json").write_text(json.dumps({
        "detector": "adaptive", "config": {}, "scenes": [
            {"id": 1, "start_time": 0.0, "end_time": 1.0,
             "start_frame": 0, "end_frame": 24},
            {"id": 2, "start_time": 1.0, "end_time": 2.0,
             "start_frame": 24, "end_frame": 48},
        ],
    }))
    m = new_manifest(slug="ws", source=SourceInfo(
        relative_path="x", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    ctx = StageContext(
        workspace_dir=ws, working_root=tmp_path, source_path=ws / "x",
        manifest=m, config=Config(), logger=logging.getLogger("t"),
        cancel_event=threading.Event(),
    )
    return ctx

def test_unchanged_file_stays_awaiting(tmp_path):
    ctx = _setup(tmp_path)
    first = ReviewStage().run(ctx)
    ctx.manifest.stages["review"].status = "awaiting_user"
    ctx.manifest.stages["review"].artifacts = first.artifacts
    second = ReviewStage().run(ctx)
    assert second.status == "awaiting_user"

def test_user_edit_advances(tmp_path):
    ctx = _setup(tmp_path)
    first = ReviewStage().run(ctx)
    ctx.manifest.stages["review"].status = "awaiting_user"
    ctx.manifest.stages["review"].artifacts = first.artifacts
    cuts_path = ctx.workspace_dir / "decisions" / "cuts.json"
    data = json.loads(cuts_path.read_text())
    data["scenes"][1]["keep"] = False
    cuts_path.write_text(json.dumps(data, indent=2) + "\n")
    second = ReviewStage().run(ctx)
    assert second.status == "done"
    assert second.artifacts["scenes_marked_keep_final"] == 1

def test_approve_flag_advances_unchanged(tmp_path):
    ctx = _setup(tmp_path)
    first = ReviewStage().run(ctx)
    ctx.manifest.stages["review"].status = "awaiting_user"
    ctx.manifest.stages["review"].artifacts = {
        **first.artifacts,
        "approved_at": datetime.now(timezone.utc).isoformat(),
    }
    second = ReviewStage().run(ctx)
    assert second.status == "done"

def test_template_auto_apply(tmp_path):
    ctx = _setup(tmp_path)
    template = tmp_path / "cuts.json.template"
    template.write_text(json.dumps({
        "schema_version": 1,
        "scenes": [
            {"id": 1, "start": "00:00:00.000", "end": "00:00:01.000", "keep": True},
            {"id": 2, "start": "00:00:01.000", "end": "00:00:02.000", "keep": False},
        ],
    }))
    result = ReviewStage().run(ctx)
    assert result.status == "done"
    data = json.loads((ctx.workspace_dir / "decisions" / "cuts.json").read_text())
    assert data["scenes"][1]["keep"] is False

def test_missing_id_raises(tmp_path):
    import pytest
    ctx = _setup(tmp_path)
    first = ReviewStage().run(ctx)
    ctx.manifest.stages["review"].status = "awaiting_user"
    ctx.manifest.stages["review"].artifacts = first.artifacts
    cuts_path = ctx.workspace_dir / "decisions" / "cuts.json"
    data = json.loads(cuts_path.read_text())
    data["scenes"].pop()       # remove a scene
    cuts_path.write_text(json.dumps(data, indent=2) + "\n")
    with pytest.raises(ValueError, match="use keep:false"):
        ReviewStage().run(ctx)
