from __future__ import annotations
import logging
import threading
from datetime import datetime, timezone
from pathlib import Path
from vsplit.core.manifest import load_manifest, save_manifest
from vsplit.core.tool_versions import probe_versions
from vsplit.core.types import Stage, StageContext
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import StageError, ToolVersions


def _close_logger(logger: logging.Logger) -> None:
    """Close and detach all handlers so the underlying log file is released.

    A FileHandler keeps its file open until closed. In the long-lived dashboard
    process this leaked one open handle per stage run, and on Windows an open
    handle blocks deletion — so `clean` (shutil.rmtree) silently left each run
    workspace's logs/ dir, and thus the whole folder, behind.
    """
    for h in list(logger.handlers):
        h.close()
        logger.removeHandler(h)


def _stage_logger(workspace_dir: Path, name: str) -> logging.Logger:
    logs = workspace_dir / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    logger = logging.getLogger(f"vsplit.{workspace_dir.name}.{name}")
    logger.setLevel(logging.DEBUG)
    # Drop any prior handlers (test isolation / re-run)
    _close_logger(logger)
    handler = logging.FileHandler(logs / f"{name}.log", encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
    logger.addHandler(handler)
    logger.propagate = False
    return logger


def run_stage(
    *,
    stage: Stage,
    workspace_dir: Path,
    working_root: Path,
    source_path: Path,
    config: Config,
    cancel_event: threading.Event,
) -> None:
    """Run one stage, persist status before and after."""
    manifest = load_manifest(workspace_dir)
    versions = probe_versions()
    manifest.tool_versions = ToolVersions(**versions)
    manifest.config_snapshot = {
        "detect": config.detect.model_dump(),
        "split": config.split.model_dump(),
        "review": {"default_keep": config.review.default_keep},
    }
    block = manifest.stages[stage.name]
    block.status = "running"
    block.started_at = datetime.now(timezone.utc)
    block.error = None
    save_manifest(workspace_dir, manifest)

    logger = _stage_logger(workspace_dir, stage.name)
    ctx = StageContext(
        workspace_dir=workspace_dir,
        working_root=working_root,
        source_path=source_path,
        manifest=manifest,
        config=config,
        logger=logger,
        cancel_event=cancel_event,
    )

    try:
        try:
            result = stage.run(ctx)
        except Exception as exc:
            logger.exception("stage failed")
            block.status = "failed"
            block.error = StageError(
                message=str(exc),
                exit_code=getattr(exc, "returncode", None),
                log_path=str((workspace_dir / "logs" / f"{stage.name}.log").relative_to(working_root)),
            )
            save_manifest(workspace_dir, manifest)
            return

        block.status = result.status
        if result.status == "awaiting_user":
            block.set_at = datetime.now(timezone.utc)
        else:
            block.completed_at = datetime.now(timezone.utc)
        if result.artifacts:
            block.artifacts.update(result.artifacts)
        save_manifest(workspace_dir, manifest)
    finally:
        # Release the per-stage log file handle so the workspace can be deleted
        # later. Without this, the long-lived dashboard process holds logs/*.log
        # open and `clean` leaves the folder behind on Windows.
        _close_logger(logger)
