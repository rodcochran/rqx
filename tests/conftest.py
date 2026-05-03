import asyncio
import ssl
import subprocess
import threading
from collections import defaultdict
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

import pytest
from aiohttp import web

DEFAULT_ERRORS_BEFORE_SUCCESS = 3


class FlakyServerHandler(BaseHTTPRequestHandler):
    counters = defaultdict(int)  # shared across requests

    def do_GET(self):
        # parse path like /flaky/3?request_id=abc
        # increment counters[request_id]
        # if count <= fail_count: send 503
        # else: send 200 with JSON body

        parsed = urlparse(self.path)
        path = parsed.path  # e.g. "/flaky/3"
        params = parse_qs(parsed.query)  # e.g. {"request_id": ["abc"]}

        request_id = params["request_id"][0]

        self.counters[request_id] += 1

        if self.counters[request_id] < DEFAULT_ERRORS_BEFORE_SUCCESS:
            # For a 503:
            self.send_response(503)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status": "failing"}')
            print("FLAKY API returned 503")

        else:
            # For a 200:
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status": "ok"}')
            print("FLAKY API returned 200")


@pytest.fixture(scope="session")
def flaky_server():
    # start server in thread
    server = HTTPServer(("localhost", 0), FlakyServerHandler)
    port = server.server_address[1]  # get the assigned port
    thread = threading.Thread(target=server.serve_forever)
    thread.daemon = True
    thread.start()
    yield f"http://localhost:{port}"
    server.shutdown()


class MTLSHandler:
    async def handle(self, request: web.Request):
        return web.Response(status=200)


@pytest.fixture(scope="session")
def mtls_server():

    ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ssl_context.load_cert_chain(certfile="cert.pem", keyfile="key.pem")

    # whose-signed-clients-do-I-trust
    ssl_context.load_verify_locations()

    # actually demand a client cert
    ssl_context.verify_mode = ssl.CERT_REQUIRED

    # generate certs
    subprocess.run(["bash", "generate_certs.sh"])

    handler = MTLSHandler()
    app = web.Application()
    app.add_routes(
        [
            web.post("/", handler.handle),
        ]
    )
    app_runner = web.AppRunner(app=app)
    tcp_site = web.TCPSite(runner=app_runner, ssl_context=ssl_context, port=8888)
    thread = threading.Thread(target=asyncio.run, kwargs={"main": tcp_site.start()})
    thread.daemon = True
    thread.start()
    yield f"http://localhost:{tcp_site._port}"
    asyncio.run(tcp_site.stop())
