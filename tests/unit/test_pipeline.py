import shutil
import threading
import logging
from datetime import datetime, timezone
from pathlib import Path
import pytest
from vsplit.core.manifest import save_manifest, load_manifest, new_manifest
from vsplit.core.pipeline import run_stage
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.stages.probe import ProbeStage

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"
pytestmark = pytest.mark.skipif(not FIXTURE.exists(), reason="run make_fixtures.py")

def test_run_stage_persists_status_and_artifacts(tmp_path):
    ws = tmp_path / "ws"
    ws.mkdir()
    src = ws / FIXTURE.name
    shutil.copy(FIXTURE, src)
    m = new_manifest(
        slug="ws",
        source=SourceInfo(
            relative_path=FIXTURE.name, size_bytes=src.stat().st_size,
            mtime=datetime.now(timezone.utc), partial_hash="sha256:x",
        ),
    )
    save_manifest(ws, m)

    run_stage(
        stage=ProbeStage(),
        workspace_dir=ws,
        working_root=tmp_path,
        source_path=src,
        config=Config(),
        cancel_event=threading.Event(),
    )

    loaded = load_manifest(ws)
    assert loaded.stages["probe"].status == "done"
    assert loaded.stages["probe"].artifacts.get("keyframe_count") is not None
    assert loaded.metadata.duration_seconds is not None
