import gzip
import json
import ssl
import subprocess
import threading
import time
import zlib
from collections import defaultdict
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import brotli
import filelock
import pytest
import zstandard

# HTTP Content-Encoding header values per RFC. Brotli is the odd one out:
# the algorithm is "brotli" but the on-the-wire header value is "br".
COMPRESSION_HEADER_VALUE = {
    "gzip": "gzip",
    "deflate": "deflate",
    "brotli": "br",
    "zstd": "zstd",
}

# This gets the directory containing the script
script_dir = Path(__file__).resolve().parent

CERTS_DIR = script_dir / "ssl" / "certs"
LOCK_PATH = script_dir / "ssl" / ".cert-gen.lock"

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

        # Compressed endpoints don't need a request_id.
        if path.startswith("/compressed/"):
            algorithm = path.removeprefix("/compressed/")
            self._send_compressed(algorithm)
            return

        # /sleep/<seconds> — server waits then returns 200. Used to test ReadTimeout.
        if path.startswith("/sleep/"):
            seconds = float(path.removeprefix("/sleep/"))
            self._sleep_then_respond(seconds)
            return

        # /redirect-loop — Location header points back to itself. Used to test TooManyRedirects.
        if path == "/redirect-loop":
            self.send_response(302)
            self.send_header("Location", "/redirect-loop")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return

        request_id = params["request_id"][0]

        if path == "/reset":
            self._reset_connection(request_id)
            return

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

    def do_POST(self):

        parsed = urlparse(self.path)
        path = parsed.path  # e.g. "/flaky/3"
        params = parse_qs(parsed.query)  # e.g. {"request_id": ["abc"]}

        request_id = params["request_id"][0]

        if path == "/reset":
            self._reset_connection(request_id)
            return

        content_length = int(self.headers.get("Content-Length", 0))
        if content_length > 0:
            self.rfile.read(content_length)

        self.send_response(404)
        self.end_headers()

    def _reset_connection(self, request_id):
        self.counters[request_id] += 1
        self.connection.close()

    def _sleep_then_respond(self, seconds: float):
        time.sleep(seconds)
        body = b'{"status": "ok"}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_compressed(self, algorithm):
        payload = json.dumps({"compressed": True, "algorithm": algorithm}).encode()

        if algorithm == "gzip":
            body = gzip.compress(payload)
        elif algorithm == "deflate":
            body = zlib.compress(payload)
        elif algorithm == "brotli":
            body = brotli.compress(payload)
        elif algorithm == "zstd":
            body = zstandard.ZstdCompressor().compress(payload)
        else:
            self.send_response(404)
            self.end_headers()
            return

        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Encoding", COMPRESSION_HEADER_VALUE[algorithm])
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


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


class MTLSHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        body = b'{"status": "ok"}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        print("MTLS API returned 200")


@pytest.fixture(scope="session")
def mtls_server():

    # generate certs
    with filelock.FileLock(str(LOCK_PATH)):
        if not (CERTS_DIR / "client-cert.pem").exists():
            subprocess.run(
                ["bash", f"{script_dir}/ssl/generate_certs.sh"],
                check=True,
            )

    ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ssl_context.load_cert_chain(
        certfile=f"{script_dir}/ssl/certs/server-cert.pem",
        keyfile=f"{script_dir}/ssl/certs/server-key.pem",
    )

    # whose-signed-clients-do-I-trust
    ssl_context.load_verify_locations(cafile=f"{script_dir}/ssl/certs/ca-cert.pem")

    # actually demand a client cert
    ssl_context.verify_mode = ssl.CERT_REQUIRED

    server = HTTPServer(("localhost", 0), MTLSHandler)
    server.socket = ssl_context.wrap_socket(server.socket, server_side=True)
    port = server.server_address[1]  # get the assigned port
    thread = threading.Thread(target=server.serve_forever)
    thread.daemon = True
    thread.start()
    yield f"https://localhost:{port}"
    server.shutdown()
