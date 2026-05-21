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


def test_run_stage_does_not_leak_log_handles(tmp_path):
    """Repeated stage runs must close prior FileHandlers so the .log file can be re-created."""
    import logging
    from vsplit.core.pipeline import _stage_logger
    ws = tmp_path / "ws"
    ws.mkdir()
    # First call: capture the handler that gets opened.
    logger1 = _stage_logger(ws, "probe")
    logger1.info("first")
    first_handler = logger1.handlers[0]
    assert isinstance(first_handler, logging.FileHandler)
    # Second call: _stage_logger must close the prior handler before removing it.
    logger2 = _stage_logger(ws, "probe")  # should close first_handler
    logger2.info("second")
    # The prior handler must be closed — its stream should be None or closed.
    assert first_handler.stream is None or first_handler.stream.closed, (
        "prior FileHandler was not closed — file handle leaked"
    )
    # Exactly one handler remains (the new one).
    assert len(logger2.handlers) == 1
    log_path = ws / "logs" / "probe.log"
    assert log_path.exists()
    # Cleanup remaining handler then delete the file — should succeed on Windows.
    for h in list(logger2.handlers):
        h.close()
        logger2.removeHandler(h)
    log_path.unlink()  # raises PermissionError on Windows if any handle is still open
    assert not log_path.exists()
