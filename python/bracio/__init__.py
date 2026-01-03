from .bracio import BracioLoop as _Loop

import asyncio
from asyncio import BaseEventLoop


class BracioLoop(_Loop, BaseEventLoop):
    def __init__(self, debug=None):
        BaseEventLoop.__init__(self)
        if debug is not None:
            self.set_debug(debug)

    async def shutdown_asyncgens(self):
        pass


def run(coro, *, debug=None):
    loop = BracioLoop()
    if debug is not None:
        loop.set_debug(debug)

    try:
        asyncio.set_event_loop(loop)
        return loop.run_until_complete(coro)
    finally:
        try:
            tasks = asyncio.all_tasks(loop)
            for task in tasks:
                task.cancel()

            if tasks:
                loop.run_until_complete(loop.shutdown_asyncgens())
        except Exception:
            pass

        asyncio.set_event_loop(None)
        loop.close()


__all__ = ["BracioLoop", "run"]
