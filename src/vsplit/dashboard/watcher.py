"""Background thread that watches per-workspace manifest.json mtimes and
publishes change events to the SSE broker.

This catches edits made by:
- another `vsplit` CLI invocation
- manual JSON edits
- the dashboard's own action handlers (redundant but harmless)
"""
from __future__ import annotations
import asyncio
import threading
import time
from pathlib import Path

from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest, manifest_path
from vsplit.core.paths import workspace_dir
from vsplit.dashboard.sse import EventBroker


class ManifestWatcher:
    def __init__(
        self,
        working_root: Path,
        broker: EventBroker,
        *,
        poll_interval: float = 0.5,
        loop: asyncio.AbstractEventLoop | None = None,
    ) -> None:
        self._root = working_root
        self._broker = broker
        self._interval = poll_interval
        self._loop = loop
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._mtimes: dict[str, float] = {}
        self._known: set[str] = set()

    def start(self) -> None:
        self._loop = self._loop or asyncio.get_event_loop()
        self._thread = threading.Thread(target=self._loop_run, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=2.0)

    def _publish(self, event: dict) -> None:
        # publish() is sync; safe to call from background thread when the
        # asyncio.Queue itself is not bound to a specific loop. EventBroker's
        # queues use put_nowait so this is fine.
        self._broker.publish(event)

    def _loop_run(self) -> None:
        while not self._stop.is_set():
            try:
                self._tick()
            except Exception:  # noqa: BLE001 — watcher must never crash the server
                pass
            time.sleep(self._interval)

    def _tick(self) -> None:
        try:
            idx = load_index(self._root)
        except Exception:
            return
        current = set(idx.workspaces.keys())
        added = current - self._known
        removed = self._known - current
        if added:
            self._publish({"type": "workspaces_added", "slugs": sorted(added)})
        if removed:
            self._publish({"type": "workspaces_removed", "slugs": sorted(removed)})
            for s in removed:
                self._mtimes.pop(s, None)
        self._known = current

        for slug in current:
            p = manifest_path(workspace_dir(self._root, slug))
            try:
                mt = p.stat().st_mtime
            except FileNotFoundError:
                continue
            prev = self._mtimes.get(slug)
            if prev != mt:
                self._mtimes[slug] = mt
                try:
                    m = load_manifest(workspace_dir(self._root, slug))
                except Exception:
                    continue
                self._publish({
                    "type": "workspace_updated",
                    "slug": slug,
                    "stages": {n: b.status for n, b in m.stages.items()},
                    "updated_at": m.updated_at.isoformat(),
                })
