import asyncio
import pytest
from vsplit.dashboard.sse import EventBroker


@pytest.mark.asyncio
async def test_broker_delivers_events_to_subscriber() -> None:
    broker = EventBroker()
    received = []

    async def consume():
        async for ev in broker.subscribe():
            received.append(ev)
            if len(received) == 2:
                break

    task = asyncio.create_task(consume())
    await asyncio.sleep(0)  # let subscriber register
    broker.publish({"type": "a"})
    broker.publish({"type": "b"})
    await asyncio.wait_for(task, timeout=1.0)
    assert received == [{"type": "a"}, {"type": "b"}]


@pytest.mark.asyncio
async def test_broker_supports_multiple_subscribers() -> None:
    broker = EventBroker()

    async def consume(buf: list):
        async for ev in broker.subscribe():
            buf.append(ev)
            if len(buf) == 1:
                break

    a, b = [], []
    t1 = asyncio.create_task(consume(a))
    t2 = asyncio.create_task(consume(b))
    await asyncio.sleep(0)
    broker.publish({"type": "x"})
    await asyncio.gather(asyncio.wait_for(t1, 1.0), asyncio.wait_for(t2, 1.0))
    assert a == [{"type": "x"}]
    assert b == [{"type": "x"}]
