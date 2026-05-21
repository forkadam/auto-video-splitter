import shutil
import threading
import logging
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
import pytest
from vsplit.core.manifest import new_manifest
from vsplit.core.types import StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.stages.split import SplitStage

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_split_produces_outputs(tmp_path):
    ws = tmp_path / "ws"
    ws.mkdir()
    src = ws / FIXTURE.name
    shutil.copy(FIXTURE, src)
    (ws / "plan").mkdir()
    (ws / "plan" / "cuts-resolved.json").write_text(json.dumps({
        "scenes": [
            {"id": 1, "label": None, "requested_start": 0.0, "requested_end": 2.0,
             "actual_start": 0.0, "actual_end": 2.0, "drift_seconds": 0.0,
             "strategy": "stream_copy", "output_filename": "ws-highlight-01.mp4"},
            {"id": 2, "label": "tail", "requested_start": 2.0, "requested_end": 4.0,
             "actual_start": 2.0, "actual_end": 4.0, "drift_seconds": 0.0,
             "strategy": "stream_copy", "output_filename": "ws-highlight-02-tail.mp4"},
        ],
    }))
    m = new_manifest(slug="ws", source=SourceInfo(
        relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    ctx = StageContext(
        workspace_dir=ws, working_root=tmp_path, source_path=src,
        manifest=m, config=Config(), logger=logging.getLogger("t"),
        cancel_event=threading.Event(),
    )
    result = SplitStage().run(ctx)
    assert result.status == "done"
    outs = list((ws / "output").glob("*.mp4"))
    assert len(outs) == 2
    for o in outs:
        assert o.stat().st_size > 0
