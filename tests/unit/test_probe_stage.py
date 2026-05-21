import shutil
import subprocess
import threading
import logging
from datetime import datetime, timezone
from pathlib import Path
import pytest
from vsplit.core.manifest import save_manifest, new_manifest
from vsplit.core.types import StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo
from vsplit.stages.probe import ProbeStage

FIXTURE = Path(__file__).parents[1] / "fixtures" / "short.mp4"

pytestmark = pytest.mark.skipif(
    not FIXTURE.exists(),
    reason="Run tests/fixtures/make_fixtures.py first",
)

def _ctx(tmp_path: Path) -> StageContext:
    workspace = tmp_path / "ws"
    workspace.mkdir()
    src = workspace / FIXTURE.name
    shutil.copy(FIXTURE, src)
    m = new_manifest(
        slug="ws",
        source=SourceInfo(
            relative_path=FIXTURE.name,
            size_bytes=src.stat().st_size,
            mtime=datetime.now(timezone.utc),
            partial_hash="sha256:x",
        ),
    )
    return StageContext(
        workspace_dir=workspace,
        working_root=tmp_path,
        source_path=src,
        manifest=m,
        config=Config(),
        logger=logging.getLogger("test"),
        cancel_event=threading.Event(),
    )

def test_probe_writes_metadata_and_artifacts(tmp_path):
    ctx = _ctx(tmp_path)
    result = ProbeStage().run(ctx)
    assert result.status == "done"
    assert (ctx.workspace_dir / "probe" / "ffprobe.json").exists()
    assert (ctx.workspace_dir / "probe" / "keyframes.csv").exists()
    assert ctx.manifest.metadata.duration_seconds is not None
    assert ctx.manifest.metadata.fps is not None
    assert ctx.manifest.metadata.keyframe_count is not None
