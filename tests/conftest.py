import threading
from collections import defaultdict
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

import pytest

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
