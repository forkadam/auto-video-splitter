import shutil
import threading
import logging
from datetime import datetime, timezone
from pathlib import Path
import json
import pytest
from vsplit.core.manifest import save_manifest, new_manifest
from vsplit.core.types import StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.stages.detect import DetectStage

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_detect_finds_scenes(tmp_path):
    ws = tmp_path / "ws"
    ws.mkdir()
    src = ws / FIXTURE.name
    shutil.copy(FIXTURE, src)
    m = new_manifest(
        slug="ws",
        source=SourceInfo(relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
                          mtime=datetime.now(timezone.utc), partial_hash="x"),
    )
    save_manifest(ws, m)
    ctx = StageContext(
        workspace_dir=ws, working_root=tmp_path, source_path=src,
        manifest=m, config=Config(), logger=logging.getLogger("test"),
        cancel_event=threading.Event(),
    )
    result = DetectStage().run(ctx)
    assert result.status == "done"
    data = json.loads((ws / "detect" / "scenes.json").read_text())
    assert data["detector"] == "adaptive"
    assert len(data["scenes"]) >= 2     # fixture has explicit color changes
    assert all("start_time" in s and "end_time" in s for s in data["scenes"])
