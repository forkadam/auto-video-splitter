"""run_stage must release its per-stage log FileHandler after each run.

A leaked FileHandler keeps workspace/logs/<stage>.log open for the life of the
process. On Windows an open handle blocks deletion, so `clean` (shutil.rmtree)
silently left the workspace's logs/ dir — and thus the whole workspace folder —
behind. This is a hermetic test: a no-op stage + monkeypatched tool probe, so it
needs neither the video fixture nor ffmpeg.
"""
import logging
import shutil
import threading
from datetime import datetime, timezone

from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.pipeline import run_stage
from vsplit.core.types import StageResult
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import SourceInfo


class _NoopStage:
    name = "probe"
    depends_on: list[str] = []
    output_subdir = None

    def run(self, ctx) -> StageResult:
        ctx.logger.info("noop ran")
        return StageResult(artifacts={})


def test_run_stage_closes_log_handler_after_single_run(tmp_path, monkeypatch) -> None:
    monkeypatch.setattr("vsplit.core.pipeline.probe_versions", lambda: {})
    ws = tmp_path / "ws"
    ws.mkdir()
    m = new_manifest(slug="ws", source=SourceInfo(
        relative_path="ws.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)

    run_stage(
        stage=_NoopStage(),
        workspace_dir=ws,
        working_root=tmp_path,
        source_path=ws / "ws.mp4",
        config=Config(),
        cancel_event=threading.Event(),
    )

    logger = logging.getLogger(f"vsplit.{ws.name}.probe")
    assert logger.handlers == [], "run_stage leaked an open log FileHandler"
    # The open handle would block deleting logs/ on Windows; prove it's released.
    shutil.rmtree(ws / "logs")
    assert not (ws / "logs").exists()
