import os
import bracio
import asyncio


def test_add_reader_pipe():
    results = []

    async def main():
        loop = asyncio.get_running_loop()
        r, w = os.pipe()

        # callback should be called when data is available
        def on_readable():
            try:
                os.read(r, 1024)
            except Exception:
                pass
            results.append("read")

        loop.add_reader(r, on_readable, ())

        os.write(w, b"x")

        await asyncio.sleep(0.05)

        loop.remove_reader(r)
        os.close(r)
        os.close(w)

    bracio.run(main())
    assert results == ["read"]
