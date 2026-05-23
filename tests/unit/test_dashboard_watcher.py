import asyncio
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

import pytest

from vsplit.core.index import save_index
from vsplit.core.manifest import new_manifest, save_manifest
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.sse import EventBroker
from vsplit.dashboard.watcher import ManifestWatcher
from vsplit.schemas.index import Index, WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo


def _seed(tmp_path: Path, slug: str):
    ws = workspace_dir(tmp_path, slug)
    ws.mkdir(parents=True)
    m = new_manifest(slug=slug, source=SourceInfo(
        relative_path=f"{slug}.mp4", size_bytes=1,
        mtime=datetime.now(timezone.utc), partial_hash="x"))
    save_manifest(ws, m)
    return ws, m


async def test_watcher_emits_on_manifest_change(tmp_path: Path) -> None:
    ws, m = _seed(tmp_path, "alpha")
    save_index(tmp_path, Index(workspaces={"alpha": WorkspaceEntry(
        source_relative_path="alpha.mp4", size_bytes=1,
        partial_hash="x", adopted_at=datetime.now(timezone.utc),
    )}))
    broker = EventBroker()
    watcher = ManifestWatcher(tmp_path, broker, poll_interval=0.05)
    watcher.start()
    try:
        sub = broker.subscribe()
        # Trigger a manifest change
        await asyncio.sleep(0.1)
        m.stages["probe"].status = "done"
        save_manifest(ws, m)
        ev = await asyncio.wait_for(anext(sub), timeout=2.0)
        assert ev["type"] == "workspace_updated"
        assert ev["slug"] == "alpha"
    finally:
        watcher.stop()
