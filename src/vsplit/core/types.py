from __future__ import annotations
import logging
import shutil
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal

from vsplit.schemas.config import Config
from vsplit.schemas.manifest import Manifest


@dataclass
class StageContext:
    workspace_dir: Path
    working_root: Path
    source_path: Path
    manifest: Manifest
    config: Config
    logger: logging.Logger
    cancel_event: threading.Event


@dataclass
class StageResult:
    artifacts: dict[str, Any] = field(default_factory=dict)
    status: Literal["done", "awaiting_user"] = "done"


class Stage:
    name: str
    depends_on: list[str] = []
    output_subdir: str | None = None

    def run(self, ctx: StageContext) -> StageResult:
        raise NotImplementedError

    def recover(self, ctx: StageContext) -> None:
        """Default: delete the stage's output_subdir, reset status to pending."""
        if self.output_subdir is not None:
            shutil.rmtree(ctx.workspace_dir / self.output_subdir, ignore_errors=True)
        block = ctx.manifest.stages[self.name]
        block.status = "pending"
        block.artifacts = {}
        block.error = None
        block.started_at = None
        block.completed_at = None
