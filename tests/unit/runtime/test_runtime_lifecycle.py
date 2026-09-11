"""Runtime lifecycle: lazy construction, survival across fork(), clean interpreter exit.

Regression coverage for https://github.com/rodcochran/rqx/issues/159 (tokio runtime built at import did not survive fork)
and https://github.com/rodcochran/rqx/issues/99 (SIGABRT at interpreter shutdown under async load).

Every scenario runs in a fresh subprocess: the behaviors under test are process
lifecycle events (import, fork, exit), which cannot be observed from inside the
pytest process itself.
"""

import os
import subprocess
import sys
import textwrap
from dataclasses import dataclass

import pytest

SUBPROCESS_TIMEOUT = 60

HAS_FORK = hasattr(os, "fork") and sys.platform != "win32"


@dataclass(frozen=True)
class Run:
    returncode: int
    stdout: str
    stderr: str

    @property
    def crashed(self) -> bool:
        # Negative = killed by signal (SIGABRT is -6); 134/139 = shell-style abort/segv.
        return self.returncode < 0 or self.returncode in (134, 139)


def run_script(source: str, *args: str, timeout: float = SUBPROCESS_TIMEOUT) -> Run:
    proc = subprocess.run(  # noqa: S603 - fixed interpreter, test-owned script source
        [sys.executable, "-X", "faulthandler", "-c", textwrap.dedent(source), *args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    return Run(proc.returncode, proc.stdout, proc.stderr)


def assert_clean_exit(run: Run) -> None:
    assert not run.crashed, f"process crashed (exit {run.returncode}):\n{run.stderr}"
    assert run.returncode == 0, f"exit {run.returncode}:\n{run.stderr}"
    assert "Fatal Python error" not in run.stderr, run.stderr
    assert "panicked" not in run.stderr, run.stderr


# ── import ────────────────────────────────────────────────────────────────


OS_THREAD_COUNT = """
    import os, subprocess, sys

    def os_threads():
        if os.path.isdir("/proc/self/task"):
            return len(os.listdir("/proc/self/task"))
        if sys.platform == "darwin":
            out = subprocess.run(["ps", "-M", "-p", str(os.getpid())], capture_output=True, text=True).stdout
            return len(out.strip().splitlines()) - 1
        return None

    before = os_threads()
    import rqx
    after = os_threads()
    print(before, after)
"""


def test_import_does_not_start_runtime_threads():
    """https://github.com/rodcochran/rqx/issues/159: the runtime must not exist until first use, so `import rqx` alone
    leaves nothing behind for fork() to break."""
    run = run_script(OS_THREAD_COUNT)
    assert_clean_exit(run)
    before, after = run.stdout.split()
    if before == "None":
        pytest.skip("no OS thread count available on this platform")
    assert int(after) == int(before), (
        f"import rqx started {int(after) - int(before)} OS threads"
    )


def test_import_and_exit_without_use():
    """The atexit shutdown hook must be a no-op when the runtime was never built."""
    assert_clean_exit(run_script("import rqx"))


# ── fork ──────────────────────────────────────────────────────────────────


FORK_SCRIPT = """
    import asyncio, multiprocessing as mp, sys
    import rqx

    url, warm, mode = sys.argv[1], sys.argv[2] == "warm", sys.argv[3]

    if warm:
        # Build the runtime in the parent: the shared-client / warmup-request shape.
        assert rqx.get(url).status_code == 200

    def child_sync():
        with rqx.Client(timeout=10.0) as c:
            assert c.get(url).status_code == 200

    def child_async():
        async def go():
            async with rqx.AsyncClient(timeout=10.0) as c:
                return (await c.get(url)).status_code
        assert asyncio.run(go()) == 200

    target = child_async if mode == "async" else child_sync
    p = mp.get_context("fork").Process(target=target)
    p.start()
    p.join(timeout=30)
    if p.is_alive():
        p.kill()
        p.join()
        sys.exit("child hung")
    sys.exit(0 if p.exitcode == 0 else f"child exit code {p.exitcode}")
"""


@pytest.mark.skipif(not HAS_FORK, reason="fork() not available")
@pytest.mark.parametrize("warm", ["cold", "warm"])
@pytest.mark.parametrize("mode", ["sync", "async"])
def test_fork_child_can_make_requests(flaky_server, warm, mode):
    """https://github.com/rodcochran/rqx/issues/159: a child forked after `import rqx` (and optionally after the parent
    already made a request) must be able to make its own requests."""
    run = run_script(FORK_SCRIPT, f"{flaky_server}/sleep/0", warm, mode)
    assert_clean_exit(run)


# ── interpreter exit ──────────────────────────────────────────────────────


INFLIGHT_AT_EXIT = """
    import asyncio, atexit, io, os, sys, time
    import rqx

    url = sys.argv[1]

    class SlowRaw(io.RawIOBase):
        # Holds the BufferedWriter lock for a while on every write, so a foreign
        # thread printing during shutdown collides with CPython's final flush.
        def __init__(self, fd): self.fd = fd
        def writable(self): return True
        def write(self, b):
            n = os.write(self.fd, bytes(b)); time.sleep(0.05); return n

    sys.stderr = io.TextIOWrapper(io.BufferedWriter(SlowRaw(2)), write_through=True, line_buffering=True)
    # Delay exit so the in-flight response lands after asyncio.run() closed the loop.
    atexit.register(lambda: time.sleep(0.6))

    async def main():
        client = rqx.AsyncClient(timeout=10.0)
        assert (await client.get(url)).status_code == 200
        client.get(url.replace("/sleep/0", "/sleep/0.5"))  # never awaited, never cancelled

    asyncio.run(main())
"""


def test_exit_with_future_in_flight_is_clean(flaky_server):
    """https://github.com/rodcochran/rqx/issues/99: a Rust future still running when asyncio.run() returns must not be
    allowed to reach into a finalizing interpreter. Before the fix this
    aborted (macOS, `_enter_buffered_busy`) or segfaulted (Linux)."""
    run = run_script(INFLIGHT_AT_EXIT, f"{flaky_server}/sleep/0")
    assert_clean_exit(run)


ASYNC_LOAD_THEN_EXIT = """
    import asyncio, sys, time
    import rqx

    url, conc, secs = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])
    done = 0

    async def worker(client, stop):
        global done
        while time.monotonic() < stop:
            try:
                assert (await client.get(url)).status_code == 200
                done += 1
            except rqx.RqxError:
                pass  # the single-threaded test server drops connections under load

    async def main():
        stop = time.monotonic() + secs
        async with rqx.AsyncClient(timeout=2.0) as client:
            await asyncio.gather(*(worker(client, stop) for _ in range(conc)))

    asyncio.run(main())
    print(done)
"""


def test_exit_after_sustained_async_load_is_clean(flaky_server):
    """https://github.com/rodcochran/rqx/issues/99, bench-shaped: sustained concurrent async load, then a normal exit."""
    # Modest load: the fixture server is single-threaded, and this test is about
    # what happens after the load, not the server's capacity.
    run = run_script(ASYNC_LOAD_THEN_EXIT, f"{flaky_server}/sleep/0", "4", "0.5")
    assert_clean_exit(run)
    assert int(run.stdout.strip()) > 0, "no request completed; the load phase never ran"


CANCELLED_LOAD_THEN_EXIT = """
    import asyncio, sys
    import rqx

    url = sys.argv[1]

    async def main():
        async with rqx.AsyncClient(timeout=10.0) as client:
            tasks = [asyncio.ensure_future(client.get(url)) for _ in range(20)]
            await asyncio.sleep(0.05)
            for t in tasks:
                t.cancel()

    asyncio.run(main())
"""


def test_exit_with_cancelled_requests_is_clean(flaky_server):
    """https://github.com/rodcochran/rqx/issues/99: requests cancelled from Python while their Rust side is still
    running must not disturb interpreter exit."""
    run = run_script(CANCELLED_LOAD_THEN_EXIT, f"{flaky_server}/sleep/0.3")
    assert_clean_exit(run)
