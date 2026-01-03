import asyncio
import bracio


def test_sleep_and_return():
    async def main():
        await asyncio.sleep(0.01)
        return "success"

    result = bracio.run(main())
    assert result == "success"


def test_scheduling_order():
    results = []

    async def main():
        loop = asyncio.get_running_loop()

        loop.call_later(0.1, lambda: results.append("timer"))
        loop.call_soon(lambda: results.append("soon"))

        await asyncio.sleep(0.2)

    bracio.run(main())
    assert results == ["soon", "timer"]


def test_cancellation():
    async def main():
        loop = asyncio.get_running_loop()
        canary = []

        handle = loop.call_later(0.05, lambda: canary.append("dead"))
        handle.cancel()

        await asyncio.sleep(0.1)
        return canary

    result = bracio.run(main())
    assert result == []
