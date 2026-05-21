import threading
from pathlib import Path
from datetime import datetime, timezone
import logging
from vsplit.core.types import Stage, StageContext, StageResult
from vsplit.schemas.manifest import Manifest, SourceInfo, StageBlock
from vsplit.schemas.config import Config

def _ctx(workspace: Path, name: str) -> StageContext:
    workspace.mkdir(parents=True, exist_ok=True)
    (workspace / name).mkdir(exist_ok=True)
    (workspace / name / "leftover.txt").write_text("partial")
    m = Manifest(
        id="x",
        created_at=datetime.now(timezone.utc),
        updated_at=datetime.now(timezone.utc),
        source=SourceInfo(
            relative_path="x", size_bytes=1, mtime=datetime.now(timezone.utc), partial_hash="x"
        ),
        stages={n: StageBlock(status="running" if n == name else "pending")
                for n in ("probe", "detect", "preview", "review", "plan", "split", "verify")},
    )
    return StageContext(
        workspace_dir=workspace,
        working_root=workspace.parent,
        source_path=workspace / "fake.mp4",
        manifest=m,
        config=Config(),
        logger=logging.getLogger("test"),
        cancel_event=threading.Event(),
    )

class _Dummy(Stage):
    name = "probe"
    depends_on: list[str] = []
    output_subdir = "probe"

    def run(self, ctx):
        return StageResult()

def test_default_recover_deletes_subdir_and_resets_status(tmp_path):
    ctx = _ctx(tmp_path / "ws", "probe")
    stage = _Dummy()
    stage.recover(ctx)
    assert not (ctx.workspace_dir / "probe").exists()
    assert ctx.manifest.stages["probe"].status == "pending"
    assert ctx.manifest.stages["probe"].artifacts == {}


def test_default_recover_clears_set_at(tmp_path):
    ctx = _ctx(tmp_path / "ws", "probe")
    ctx.manifest.stages["probe"].set_at = datetime.now(timezone.utc)
    _Dummy().recover(ctx)
    assert ctx.manifest.stages["probe"].set_at is None
