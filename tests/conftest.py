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

DEFAULT_ERRORS_BEFORE_SUCCESS = 3


class CertSet:
    """The TLS cert set the test suite runs against.

    Owns the two questions worth asking before a run starts: is the set there,
    and is it still in date. Certs are gitignored, so answering "no" is the
    normal state of a fresh checkout, not an error.
    """

    # generate_certs.sh writes client-combined.pem last, so its presence means
    # the whole set is on disk. Checking an earlier-written file (client-cert.pem,
    # server-cert.pem) would report "certs exist" mid-generation.
    SENTINEL = "client-combined.pem"

    # Every link in the chain — one stale cert fails the handshake, and the CA
    # expiring takes down every test that verifies against it.
    CHAIN = ("ca-cert.pem", "server-cert.pem", "client-cert.pem")

    # generate_certs.sh issues 365-day certs. Regenerating a day early keeps a
    # run that starts just under the wire from having one expire mid-suite.
    EXPIRY_GRACE_SECONDS = 24 * 60 * 60

    def __init__(self, directory, script, lock_path):
        self.directory = directory
        self.script = script
        self.lock_path = lock_path

    def ensure(self):
        if self.is_usable():
            return
        # The check above is deliberately unlocked — it's the common case and
        # costs three openssl calls. The lock covers generation, which is not
        # safe to run twice at once: independent pytest processes sharing a
        # checkout (two terminals, CI jobs on one workspace) interleave their
        # writes to the same files and hand each other a mismatched key pair.
        with filelock.FileLock(str(self.lock_path)):
            # Re-check under the lock. Another process may have generated while
            # we waited, and regenerating now would swap the certs out from
            # under a run that has already loaded them.
            if not self.is_usable():
                self.generate()

    def is_usable(self):
        if not (self.directory / self.SENTINEL).exists():
            return False
        return all(self.is_in_date(name) for name in self.CHAIN)

    def is_in_date(self, name):
        """`openssl x509 -checkend N` exits 0 if the cert is still valid N
        seconds from now, 1 if it has expired or will within that window.

        Without this, an existence-only check hands a year-old checkout its
        stale certs and the suite fails at handshake time with an error that
        says nothing about expiry.
        """
        path = self.directory / name
        if not path.exists():
            return False
        checked = subprocess.run(
            [
                "openssl",
                "x509",
                "-checkend",
                str(self.EXPIRY_GRACE_SECONDS),
                "-noout",
                "-in",
                str(path),
            ],
            capture_output=True,
        )
        return checked.returncode == 0

    def generate(self):
        subprocess.run(["bash", str(self.script)], check=True)


CERTS = CertSet(
    directory=CERTS_DIR,
    script=script_dir / "ssl" / "generate_certs.sh",
    # Lives beside certs/ rather than inside it: on a cold checkout the certs
    # directory doesn't exist yet, and the lock has to be creatable before the
    # script that creates the directory runs.
    lock_path=script_dir / "ssl" / ".cert-gen.lock",
)


def pytest_configure(config):
    """Generate TLS certs once, before any test process can look for them.

    The certs are gitignored, so a fresh checkout (and every CI run) starts with
    an empty tests/ssl/certs/. Generating them lazily from the server fixtures
    made the timing depend on which worker happened to draw a TLS test first:
    under `-n 8`, a test reading a cert path without requesting one of those
    fixtures could run before any cert existed and fail with "failed to read
    client cert: No such file or directory" (#112).

    Doing it here removes the race rather than guarding it. Under xdist this
    hook runs in the controller and again in each worker, but the controller's
    call completes before xdist spawns any worker, so the `workerinput` guard
    leaves exactly one process generating within a run, and no window where a
    cert is half-written when someone reads it.

    That guard says nothing about *other* pytest runs, though, so CertSet.ensure
    still takes a file lock to serialize against a second invocation sharing the
    checkout.
    """
    if hasattr(config, "workerinput"):  # xdist sets this on workers only
        return
    CERTS.ensure()


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

        # /echo-auth — echo the request's Authorization header back as JSON.
        # Used to verify that auth_bearer= sets `Authorization: Bearer <token>`.
        if path == "/echo-auth":
            self._echo_auth()
            return

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

        # /redirect-once — 302 to /streamable. Used to test follow_redirects on stream.
        if path == "/redirect-once":
            self.send_response(302)
            self.send_header("Location", "/streamable")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return

        # /streamable — final destination after /redirect-once. Returns a known body.
        if path == "/streamable":
            body = b'{"streamed": true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        # /not-modified — emits a 304 with NO Location header. Used to test
        # that resp.is_redirect is False on a 3xx that can't be followed.
        if path == "/not-modified":
            self.send_response(304)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return

        # /latin1 — returns the bytes for "café" in ISO-8859-1 (0xE9 for é),
        # advertised via Content-Type charset. Used to test resp.encoding.
        if path == "/latin1":
            body = "café".encode("iso-8859-1")
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=iso-8859-1")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        # /bigtext — ~1 MB of mixed-width multibyte UTF-8, big enough that
        # reqwest delivers it in many network chunks. Exercises the streaming
        # text decoder's cross-chunk character reassembly: with this many
        # 2/3/4-byte chars, chunk boundaries almost certainly land mid-character,
        # so a decoder that didn't hold partial bytes across __next__ calls would
        # corrupt the output.
        if path == "/bigtext":
            body = ("aé€🙂" * 100_000).encode("utf-8")  # 1+2+3+4 bytes per unit
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        # /lines — small known body with several newline-terminated lines.
        # End-to-end check for iter_lines; the cross-chunk reassembly edge cases
        # are covered deterministically by the Rust LineDecoder unit tests.
        if path == "/lines":
            body = b"first\nsecond\nthird\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
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

        if path == "/echo-auth":
            content_length = int(self.headers.get("Content-Length", 0))
            if content_length > 0:
                self.rfile.read(content_length)
            self._echo_auth()
            return

        request_id = params["request_id"][0]

        if path == "/reset":
            self._reset_connection(request_id)
            return

        content_length = int(self.headers.get("Content-Length", 0))
        if content_length > 0:
            self.rfile.read(content_length)

        self.send_response(404)
        self.end_headers()

    # PUT/PATCH/DELETE/HEAD/OPTIONS aren't auto-handled by BaseHTTPRequestHandler.
    # We only need them for /echo-auth coverage in test_auth_bearer.py.
    def do_PUT(self):
        self._handle_body_verb_for_echo_auth()

    def do_PATCH(self):
        self._handle_body_verb_for_echo_auth()

    def do_DELETE(self):
        self._handle_simple_verb_for_echo_auth()

    def do_HEAD(self):
        self._handle_simple_verb_for_echo_auth()

    def do_OPTIONS(self):
        self._handle_simple_verb_for_echo_auth()

    def _handle_body_verb_for_echo_auth(self):
        parsed = urlparse(self.path)
        if parsed.path != "/echo-auth":
            self.send_response(404)
            self.end_headers()
            return
        content_length = int(self.headers.get("Content-Length", 0))
        if content_length > 0:
            self.rfile.read(content_length)
        self._echo_auth()

    def _handle_simple_verb_for_echo_auth(self):
        parsed = urlparse(self.path)
        if parsed.path != "/echo-auth":
            self.send_response(404)
            self.end_headers()
            return
        self._echo_auth()

    def _echo_auth(self):
        auth_header = self.headers.get("Authorization", "")
        body = json.dumps({"authorization": auth_header}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        # HEAD must not include a body.
        if self.command != "HEAD":
            self.wfile.write(body)

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


# ────────────────────────────────────────────────────────────────────────
# Local HTTP/2-capable server fixture (#78).
#
# Replaces test reliance on nghttp2.org/httpbin/ for HTTP/2 negotiation
# coverage. Uses hypercorn over TLS with ALPN advertising both h2 and
# http/1.1, so the same fixture covers:
#   - ALPN-negotiated h2 (no version override on the client)
#   - explicit h2=True
#   - explicit h2=False (server falls back to h1)
#   - h2 prior knowledge (client proposes only h2 via ALPN)
# ────────────────────────────────────────────────────────────────────────


async def _http2_app(scope, receive, send):
    """Minimal ASGI app that returns 200 + {"ok": true} for any HTTP request.

    The tests only assert on .status_code and .http_version, so we don't need
    httpbin-style request echoing.
    """
    if scope["type"] != "http":
        return
    # Drain the request body — some clients won't accept a response until the
    # request body has been read.
    while True:
        msg = await receive()
        if not msg.get("more_body", False):
            break
    body = b'{"ok": true}'
    await send(
        {
            "type": "http.response.start",
            "status": 200,
            "headers": [
                (b"content-type", b"application/json"),
                (b"content-length", str(len(body)).encode()),
            ],
        }
    )
    await send({"type": "http.response.body", "body": body})


def _free_port():
    """Pick an unused localhost port. Closes the probe socket before returning,
    so there's a brief race window before hypercorn re-binds — acceptable for
    tests."""
    import socket as _socket

    s = _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


@pytest.fixture(scope="session")
def http2_server():
    """Local HTTPS server speaking both HTTP/2 and HTTP/1.1 via ALPN.

    Replaces nghttp2.org/httpbin/ for HTTP version-negotiation coverage.
    Returns a base URL like https://localhost:PORT. The TLS cert is the
    self-signed cert generated by tests/ssl/generate_certs.sh, so callers
    should pass `verify=False` (or point verify= at tests/ssl/certs/ca-cert.pem).
    """
    import asyncio

    from hypercorn.asyncio import serve
    from hypercorn.config import Config

    port = _free_port()
    config = Config()
    config.bind = [f"127.0.0.1:{port}"]
    config.certfile = str(CERTS_DIR / "server-cert.pem")
    config.keyfile = str(CERTS_DIR / "server-key.pem")
    # ALPN: advertise h2 first, then http/1.1. Clients that propose either
    # will negotiate to their preferred version.
    config.alpn_protocols = ["h2", "http/1.1"]
    # Hypercorn logs every request by default; silence for clean test output.
    config.accesslog = None
    config.errorlog = None

    shutdown = threading.Event()

    async def _shutdown_trigger():
        # Polls the threading.Event so the asyncio loop can react to a
        # cross-thread shutdown signal without blocking.
        while not shutdown.is_set():
            await asyncio.sleep(0.05)

    def _run():
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            loop.run_until_complete(
                serve(_http2_app, config, shutdown_trigger=_shutdown_trigger)
            )
        finally:
            loop.close()

    thread = threading.Thread(target=_run, daemon=True)
    thread.start()

    # Wait for the port to accept connections before yielding — otherwise the
    # first test races the server startup.
    import socket as _socket

    deadline = time.time() + 5.0
    while time.time() < deadline:
        try:
            with _socket.create_connection(("127.0.0.1", port), timeout=0.5):
                break
        except OSError:
            time.sleep(0.05)
    else:
        shutdown.set()
        raise RuntimeError(f"http2_server didn't come up on port {port}")

    try:
        yield f"https://localhost:{port}"
    finally:
        shutdown.set()
        thread.join(timeout=5.0)


@pytest.fixture(scope="session")
def mtls_server():
    # Certs are generated up front in pytest_configure.
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
