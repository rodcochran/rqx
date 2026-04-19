"""Minimal aiohttp server that simulates real network latency.

Each GET /json request sleeps for DELAY_MS before returning the payload.
Use as a benchmark target to isolate client concurrency behavior from
localhost's near-zero per-request cost.
"""

import asyncio
import os

from aiohttp import web

DELAY_MS = int(os.environ.get("DELAY_MS", "100"))
PORT = int(os.environ.get("PORT", "8081"))
PAYLOAD = b'{"id": 1, "name": "delayed_response", "latency_ms": %d}' % DELAY_MS


async def handle(request):
    await asyncio.sleep(DELAY_MS / 1000.0)
    return web.Response(body=PAYLOAD, content_type="application/json")


app = web.Application()
app.router.add_get("/json", handle)


if __name__ == "__main__":
    print(f"Delay server listening on http://localhost:{PORT}/json ({DELAY_MS}ms delay)")
    web.run_app(app, port=PORT, print=None)
