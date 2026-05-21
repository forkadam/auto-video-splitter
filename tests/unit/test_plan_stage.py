import json
import threading
import logging
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.manifest import new_manifest
from vsplit.core.types import StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.stages.plan import PlanStage


def _ctx(tmp_path: Path) -> StageContext:
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "probe").mkdir()
    # Keyframes at 0, 5, 10, 15 seconds
    (ws / "probe" / "keyframes.csv").write_text("0\n5\n10\n15\n")
    (ws / "decisions").mkdir()
    (ws / "decisions" / "cuts.json").write_text(json.dumps({
        "schema_version": 1,
        "scenes": [
            {"id": 1, "start": "00:00:01.000", "end": "00:00:06.000",
             "keep": True, "label": "intro"},
            {"id": 2, "start": "00:00:06.000", "end": "00:00:09.000",
             "keep": False},
            {"id": 3, "start": "00:00:09.000", "end": "00:00:14.000",
             "keep": True, "merge_with_next": False},
        ],
    }))
    m = new_manifest(slug="ws", source=SourceInfo(
        relative_path="x.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    return StageContext(
        workspace_dir=ws, working_root=tmp_path, source_path=ws / "x.mp4",
        manifest=m, config=Config(), logger=logging.getLogger("t"),
        cancel_event=threading.Event(),
    )


def test_plan_snaps_to_preceding_keyframe(tmp_path):
    ctx = _ctx(tmp_path)
    result = PlanStage().run(ctx)
    assert result.status == "done"
    data = json.loads((ctx.workspace_dir / "plan" / "cuts-resolved.json").read_text())
    assert len(data["scenes"]) == 2     # scene 2 dropped
    s1 = data["scenes"][0]
    assert s1["id"] == 1
    assert s1["requested_start"] == 1.0
    assert s1["actual_start"] == 0.0    # snapped to preceding keyframe
    assert s1["output_filename"] == "ws-highlight-01-intro.mp4"
    s2 = data["scenes"][1]
    assert s2["id"] == 3
    assert s2["actual_start"] == 5.0    # snapped from 9.0 to 5.0
