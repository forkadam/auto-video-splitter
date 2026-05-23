"""In-process pub/sub broker for Server-Sent Events.

Subscribers each get their own asyncio.Queue. Publishers (the manifest
watcher thread, in-process action callbacks) fan-out to all queues.
"""
from __future__ import annotations
import asyncio
from typing import AsyncIterator


class EventBroker:
    def __init__(self) -> None:
        self._subscribers: list[asyncio.Queue] = []
        self._lock = asyncio.Lock()

    def publish(self, event: dict) -> None:
        """Non-blocking fan-out. Drops events for slow consumers."""
        for q in list(self._subscribers):
            try:
                q.put_nowait(event)
            except asyncio.QueueFull:
                pass  # subscriber too slow; drop

    async def subscribe(self) -> AsyncIterator[dict]:
        """Yield events for one subscriber until cancelled."""
        q: asyncio.Queue = asyncio.Queue(maxsize=256)
        self._subscribers.append(q)
        try:
            while True:
                ev = await q.get()
                yield ev
        finally:
            self._subscribers.remove(q)
