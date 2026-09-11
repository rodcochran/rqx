"""In-process test servers and the TLS cert set shared by every test kind."""

import gzip
import json
import subprocess
import threading
import time
import zlib
from collections import defaultdict
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import brotli
import filelock
import zstandard

# HTTP Content-Encoding header values per RFC. Brotli is the odd one out:
# the algorithm is "brotli" but the on-the-wire header value is "br".
COMPRESSION_HEADER_VALUE = {
    "gzip": "gzip",
    "deflate": "deflate",
    "brotli": "br",
    "zstd": "zstd",
}

# The tests/ directory, where ssl/ lives.
script_dir = Path(__file__).resolve().parent.parent

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
    #
    # Note the limit of that reasoning: "written last" only implies "complete"
    # when generating into an empty directory. Regenerating over an existing set
    # leaves the previous sentinel in place until the final write, so this check
    # is only trustworthy under the lock — see ensure().
    SENTINEL = "client-combined.pem"

    # Every link in the chain — one stale cert fails the handshake, and the CA
    # expiring takes down every test that verifies against it.
    CHAIN = ("ca-cert.pem", "server-cert.pem", "client-cert.pem")

    # The private keys the fixtures and tests open directly: server-key.pem for
    # both HTTPS servers, client-key.pem for the (cert, key) tuple tests. Only
    # their presence matters — a key has no expiry of its own — but a set
    # missing one is not usable, and without this the check calls it fine and
    # the failure lands later as a fixture error instead of a regeneration.
    KEYS = ("server-key.pem", "client-key.pem")

    # generate_certs.sh issues 365-day certs. Regenerating a day early keeps a
    # run that starts just under the wire from having one expire mid-suite.
    EXPIRY_GRACE_SECONDS = 24 * 60 * 60

    def __init__(self, directory, script, lock_path):
        self.directory = directory
        self.script = script
        self.lock_path = lock_path

    def ensure(self):
        # The check is inside the lock, not in front of it. Checking first would
        # be cheaper, but it can read a set that is halfway through being
        # replaced: regeneration overwrites the chain files before rewriting the
        # sentinel, so there is a moment when all three certs are new and in
        # date while client-combined.pem is still the previous one, signed by a
        # CA that no longer exists on disk. That set passes is_usable() and then
        # fails every handshake.
        with filelock.FileLock(str(self.lock_path)):
            # Another process may have generated while we waited, so re-check
            # rather than generating unconditionally — regenerating here would
            # swap the certs out from under a run that already loaded them.
            if not self.is_usable():
                self.generate()

    def is_usable(self):
        if not (self.directory / self.SENTINEL).exists():
            return False
        if not all((self.directory / name).exists() for name in self.KEYS):
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
        # Drop the sentinel before starting. If generation dies partway — Ctrl-C,
        # a CI timeout, any openssl failure now that the script runs under
        # `set -e` — the chain files have already been replaced while
        # client-combined.pem still belongs to the previous set. is_usable()
        # can't see that mismatch, since it only asks whether files exist and
        # are in date, so the broken set would be inherited by every later run.
        # Removing the sentinel first makes an interrupted generation look
        # exactly like no generation at all.
        (self.directory / self.SENTINEL).unlink(missing_ok=True)
        subprocess.run(["bash", str(self.script)], check=True)


CERTS = CertSet(
    directory=CERTS_DIR,
    script=script_dir / "ssl" / "generate_certs.sh",
    # Lives beside certs/ rather than inside it: on a cold checkout the certs
    # directory doesn't exist yet, and the lock has to be creatable before the
    # script that creates the directory runs.
    lock_path=script_dir / "ssl" / ".cert-gen.lock",
)


class QuietThreadingHTTPServer(ThreadingHTTPServer):
    """One thread per request, so a handler the client abandoned (a read-timeout
    test leaves `/sleep` running) never blocks the next request. A client that
    gave up is the expected outcome in those tests, so its broken pipe is not
    an error worth a traceback."""

    daemon_threads = True

    def handle_error(self, request, client_address):
        import sys

        exc = sys.exc_info()[1]
        if isinstance(exc, (BrokenPipeError, ConnectionResetError)):
            return
        super().handle_error(request, client_address)


class FlakyServerHandler(BaseHTTPRequestHandler):
    # TCP_NODELAY. Headers and body go out as two small sends; with Nagle on,
    # the second waits for the client's delayed ACK, which is 40 ms on Linux.
    # That was ~40 ms per request on CI (3 ms locally on macOS).
    disable_nagle_algorithm = True
    counters = defaultdict(int)  # shared across requests and handler threads

    def log_message(self, format, *args):
        pass

    counters_lock = threading.Lock()

    def _bump(self, request_id) -> int:
        """Increment and return this request_id's attempt count atomically, so
        the branch that follows sees exactly the attempt it recorded."""
        with self.counters_lock:
            self.counters[request_id] += 1
            return self.counters[request_id]

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

        if path == "/echo-body":
            self._echo_body()
            return

        # /big-ints — integer literals at and past the 64-bit boundaries, for
        # the decode side of https://github.com/rodcochran/rqx/issues/118.
        if path == "/big-ints":
            body = (
                b'{"i64_max": 9223372036854775807, "u64_min": 9223372036854775808, '
                b'"u64_max": 18446744073709551615, "past_u64": 18446744073709551617}'
            )
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        # /echo-url/<anything> — echo the path and query exactly as received.
        # Property tests generate URLs under this prefix so they never fall
        # into the flaky default below.
        if path.startswith("/echo-url"):
            body = json.dumps({"path": parsed.path, "query": parsed.query}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        # /echo-headers — echo the request headers back as a JSON list of
        # [name, value] pairs, wire order and casing preserved, duplicates kept.
        if path == "/echo-headers":
            self._echo_headers()
            return

        # /redirect/<status> — redirect with that status to /echo-body.
        if path.startswith("/redirect/") and path.removeprefix("/redirect/").isdigit():
            self._redirect(int(path.removeprefix("/redirect/")), "/echo-body")
            return

        # relative Locations on each hop; only correct if resolved against that hop.
        if path == "/nested/hop1":
            self._redirect(302, "hop2")
            return
        if path == "/nested/hop2":
            self._redirect(302, "final")
            return
        if path == "/nested/final":
            body = b"final"
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        # /redirect-loop — Location header points back to itself. Used to test TooManyRedirects.
        if path == "/redirect-loop":
            self.send_response(302)
            self.send_header("Location", "/redirect-loop")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return

        # /redirect-to-flaky — 302 to the flaky endpoint, preserving request_id.
        # Used to check that retry config still applies while following a
        # redirect chain. The Location is relative, so the client resolves it
        # against the original URL.
        if path == "/redirect-to-flaky":
            self.send_response(302)
            self.send_header("Location", f"/?request_id={params['request_id'][0]}")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return

        # /flaky-redirect — 503 for its first two hits, then a 302 to the flaky
        # endpoint under "<request_id>-dest". Two hops, two retries each.
        if path == "/flaky-redirect":
            request_id = params["request_id"][0]
            attempt = self._bump(request_id)
            if attempt < DEFAULT_ERRORS_BEFORE_SUCCESS:
                self.send_response(503)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            self.send_response(302)
            self.send_header("Location", f"/?request_id={request_id}-dest")
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
            # Written in 65537-byte slices: the unit is 10 bytes, so every
            # boundary lands mid-character (offsets 7, 4, 1, 8, ...). The
            # transport may still coalesce, but the server never helps.
            for i in range(0, len(body), 65537):
                self.wfile.write(body[i : i + 65537])
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

        # /reset-then-flaky — closes the connection on the first hit, then 503 twice, then 200.
        if path == "/reset-then-flaky":
            attempt = self._bump(request_id)
            if attempt == 1:
                self.connection.close()
                return
            if attempt <= DEFAULT_ERRORS_BEFORE_SUCCESS:
                self.send_response(503)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            self._sleep_then_respond(0)
            return

        attempt = self._bump(request_id)

        if attempt < DEFAULT_ERRORS_BEFORE_SUCCESS:
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

        if path == "/echo-body":
            self._echo_body()
            return

        # /reflect-json — send the request body back verbatim as JSON, so a
        # decode round trip sees exactly the bytes the test produced.
        if path == "/reflect-json":
            body = self._read_body()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        if path.startswith("/redirect/") and path.removeprefix("/redirect/").isdigit():
            self._read_body()
            self._redirect(int(path.removeprefix("/redirect/")), "/echo-body")
            return

        request_id = params.get("request_id", [None])[0]

        # /flaky-echo-body — 503 twice, then echoes the body (retries must resend it).
        if path == "/flaky-echo-body" and request_id is not None:
            body = self._read_body()
            attempt = self._bump(request_id)
            if attempt < DEFAULT_ERRORS_BEFORE_SUCCESS:
                self.send_response(503)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            self._echo_body(body)
            return

        if path == "/reset" and request_id is not None:
            self._reset_connection(request_id)
            return

        self._read_body()
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    # PUT/PATCH/DELETE/HEAD/OPTIONS aren't auto-handled by BaseHTTPRequestHandler.
    # Body verbs share the POST routes; the rest only need /echo-auth.
    def do_PUT(self):
        self.do_POST()

    def do_PATCH(self):
        self.do_POST()

    def do_DELETE(self):
        self._handle_simple_verb_for_echo_auth()

    def do_HEAD(self):
        self._handle_simple_verb_for_echo_auth()

    def do_OPTIONS(self):
        self._handle_simple_verb_for_echo_auth()

    def _handle_simple_verb_for_echo_auth(self):
        parsed = urlparse(self.path)
        if parsed.path != "/echo-auth":
            self.send_response(404)
            self.end_headers()
            return
        self._echo_auth()

    def _read_body(self):
        content_length = int(self.headers.get("Content-Length", 0))
        return self.rfile.read(content_length) if content_length > 0 else b""

    def _echo_body(self, body=None):
        """Echo method, Content-Type, and body as JSON."""
        if body is None:
            body = self._read_body()
        echo = json.dumps(
            {
                "method": self.command,
                "content_type": self.headers.get("Content-Type"),
                "body": body.decode("utf-8", "replace"),
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(echo)))
        self.end_headers()
        self.wfile.write(echo)

    def _redirect(self, status, location):
        self.send_response(status)
        self.send_header("Location", location)
        self.send_header("Content-Length", "0")
        self.end_headers()

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

    def _echo_headers(self):
        body = json.dumps(list(self.headers.items())).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _reset_connection(self, request_id):
        self._bump(request_id)
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


class MTLSHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    disable_nagle_algorithm = True

    def log_message(self, format, *args):
        pass

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
