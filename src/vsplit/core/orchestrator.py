from __future__ import annotations
import threading
from concurrent.futures import ThreadPoolExecutor, Future, wait, FIRST_COMPLETED
from dataclasses import dataclass
from pathlib import Path
from typing import Callable
from vsplit.core.manifest import load_manifest
from vsplit.core.pipeline import run_stage
from vsplit.core.types import Stage
from vsplit.schemas.config import Config
from vsplit.schemas.manifest import STAGE_NAMES


@dataclass(frozen=True)
class Workspace:
    slug: str
    workspace_dir: Path
    source_path: Path


def _next_runnable(manifest, stages_to_run: list[str]) -> str | None:
    for stage in stages_to_run:
        block = manifest.stages[stage]
        if block.status == "done":
            continue
        if block.status in ("failed", "awaiting_user", "running"):
            return None
        if block.status == "pending":
            # All earlier stages must be done.
            earlier = STAGE_NAMES[: STAGE_NAMES.index(stage)]
            if any(manifest.stages[e].status != "done" for e in earlier):
                return None
            return stage
        if block.status == "skipped":
            continue
    return None


def _recover_running(
    workspaces: list[Workspace],
    working_root: Path,
    config: Config,
    cancel_event: threading.Event,
    stage_factory: Callable[[str], Stage],
) -> None:
    """Before dispatch, walk each manifest and call recover() on any running stage."""
    import logging
    for ws in workspaces:
        manifest = load_manifest(ws.workspace_dir)
        for name, block in manifest.stages.items():
            if block.status != "running":
                continue
            stage = stage_factory(name)
            from vsplit.core.types import StageContext
            ctx = StageContext(
                workspace_dir=ws.workspace_dir,
                working_root=working_root,
                source_path=ws.source_path,
                manifest=manifest,
                config=config,
                logger=logging.getLogger(f"vsplit.recover.{ws.slug}.{name}"),
                cancel_event=cancel_event,
            )
            stage.recover(ctx)
        from vsplit.core.manifest import save_manifest
        save_manifest(ws.workspace_dir, manifest)


def dispatch(
    *,
    workspaces: list[Workspace],
    working_root: Path,
    config: Config,
    cancel_event: threading.Event,
    stages_to_run: list[str],
    stage_factory: Callable[[str], Stage],
) -> None:
    _recover_running(workspaces, working_root, config, cancel_event, stage_factory)

    limiters = {s: threading.Semaphore(config.concurrency.get(s, 1))
                for s in stages_to_run}
    inflight: dict[str, tuple[Future, str]] = {}
    max_workers = max(config.concurrency.get(s, 1) for s in stages_to_run) or 1

    with ThreadPoolExecutor(max_workers=max_workers * len(stages_to_run)) as ex:
        while not cancel_event.is_set():
            # Reap
            done_slugs = []
            for slug, (fut, stage_name) in list(inflight.items()):
                if fut.done():
                    try:
                        fut.result()
                    except Exception:
                        pass    # error already in manifest
                    limiters[stage_name].release()
                    done_slugs.append(slug)
            for s in done_slugs:
                del inflight[s]

            # Schedule
            scheduled = False
            for ws in workspaces:
                if ws.slug in inflight:
                    continue
                manifest = load_manifest(ws.workspace_dir)
                nxt = _next_runnable(manifest, stages_to_run)
                if nxt is None:
                    continue
                limiter = limiters[nxt]
                if not limiter.acquire(blocking=False):
                    continue
                stage = stage_factory(nxt)
                fut = ex.submit(
                    run_stage,
                    stage=stage,
                    workspace_dir=ws.workspace_dir,
                    working_root=working_root,
                    source_path=ws.source_path,
                    config=config,
                    cancel_event=cancel_event,
                )
                inflight[ws.slug] = (fut, nxt)
                scheduled = True

            if not inflight and not scheduled:
                return
            if inflight:
                wait([f for f, _ in inflight.values()],
                     return_when=FIRST_COMPLETED, timeout=0.5)
