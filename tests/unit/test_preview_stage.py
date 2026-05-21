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
from vsplit.stages.preview import PreviewStage

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_preview_generates_html_and_thumbs(tmp_path):
    ws = tmp_path / "ws"
    ws.mkdir()
    src = ws / FIXTURE.name
    shutil.copy(FIXTURE, src)
    (ws / "detect").mkdir()
    (ws / "detect" / "scenes.json").write_text(json.dumps({
        "detector": "adaptive",
        "config": {"threshold": 27.0},
        "scenes": [
            {"id": 1, "start_time": 0.0, "end_time": 1.5,
             "start_frame": 0, "end_frame": 36},
            {"id": 2, "start_time": 1.5, "end_time": 3.0,
             "start_frame": 36, "end_frame": 72},
        ],
    }))
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
    result = PreviewStage().run(ctx)
    assert result.status == "done"
    assert (ws / "preview" / "index.html").exists()
    images = list((ws / "preview" / "images").glob("scene-*.jpg"))
    assert len(images) == 6     # 2 scenes * 3 images
