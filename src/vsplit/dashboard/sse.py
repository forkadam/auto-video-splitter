"""In-process pub/sub broker for Server-Sent Events.

Subscribers each get their own asyncio.Queue. Publishers (the manifest
watcher thread, in-process action callbacks) fan-out to all queues.
"""
from __future__ import annotations
import asyncio
from typing import AsyncIterator


# Sentinel value the broker can inject into a subscriber's queue to make
# its blocking get() return immediately. Used by close_all() to wake up
# every SSE stream on server shutdown.
_SHUTDOWN_SENTINEL: dict = {"__sse_shutdown__": True}


class _QueueSubscriber:
    """Async iterator backed by a Queue.

    Using a class-based iterator (rather than an async generator) means
    each call to __anext__ creates a fresh coroutine.  asyncio.wait_for
    cancels only that coroutine on timeout — it does not close the
    iterator, so subsequent calls work correctly.  An async generator
    would be permanently closed by the first cancellation (Python 3.13).

    The queue is registered with the broker lazily on the first
    __anext__ call (not at construction time) so that events emitted
    between subscribe() and the first actual iteration are not
    buffered.  This preserves the behaviour of the previous
    async-generator implementation where the queue was appended when
    the generator body first executed.
    """

    def __init__(self, broker_list: list[asyncio.Queue]) -> None:
        self._broker_list = broker_list
        self._q: asyncio.Queue | None = None

    def _ensure_registered(self) -> asyncio.Queue:
        if self._q is None:
            self._q = asyncio.Queue(maxsize=256)
            self._broker_list.append(self._q)
        return self._q

    def __aiter__(self) -> "_QueueSubscriber":
        return self

    async def __anext__(self) -> dict:
        q = self._ensure_registered()
        return await q.get()

    async def aclose(self) -> None:
        if self._q is not None:
            try:
                self._broker_list.remove(self._q)
            except ValueError:
                pass  # already removed (e.g. double-close)
            self._q = None


class EventBroker:
    def __init__(self) -> None:
        self._subscribers: list[asyncio.Queue] = []

    def publish(self, event: dict) -> None:
        """Non-blocking fan-out. Drops events for slow consumers."""
        for q in list(self._subscribers):
            try:
                q.put_nowait(event)
            except asyncio.QueueFull:
                pass  # subscriber too slow; drop

    def subscribe(self) -> _QueueSubscriber:
        """Return an async iterator that yields events for one subscriber.

        The underlying queue is registered lazily on the first iteration
        so that events published between subscribe() and the first
        __anext__ call are not captured.
        """
        return _QueueSubscriber(self._subscribers)

    def close_all(self) -> None:
        """Inject the shutdown sentinel into every subscriber's queue so any
        blocked ``await queue.get()`` returns immediately. Lets SSE streams
        observe shutdown without racing a second awaitable (the simpler
        single-await flow is much harder to leak under cancellation)."""
        for q in list(self._subscribers):
            # Drop the oldest pending event if the queue is full so the
            # sentinel always lands — shutdown takes priority over throughput.
            while True:
                try:
                    q.put_nowait(_SHUTDOWN_SENTINEL)
                    break
                except asyncio.QueueFull:
                    try:
                        q.get_nowait()
                    except asyncio.QueueEmpty:
                        # Concurrent consumer drained it; loop will retry put.
                        continue
