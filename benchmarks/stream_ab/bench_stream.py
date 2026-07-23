"""Streaming benchmark for PR #139 / issue #108 — the per-chunk copy removal.

Deliberately lives OUTSIDE the repo checkouts. The base commit predates this
file, so it cannot be committed to either arm — the harness injects the same
script into both venvs. That is what makes it an honest A/B: identical
measurement code, only the installed wheel differs.

PRIMARY METRIC: CPU seconds per GB streamed.
    The change removes one malloc + one memcpy per chunk. That is CPU and
    allocator pressure, and CPU-time-per-byte measures it directly. Wall-clock
    throughput is reported too, but it is the WRONG headline number here: the
    workload is partly transfer-bound even over loopback, so a real CPU win
    can hide inside wall-clock noise. If the two disagree, believe CPU time.

Emits one JSON object per run on stdout for the comparison step to aggregate.
"""

import argparse
import asyncio
import resource
import threading
import time
from dataclasses import dataclass

import rqx
from records import RunRecord


class Usage:
    """Process-wide CPU and peak-RSS sampling.

    RUSAGE_SELF aggregates across threads, so this is valid for both the
    thread-pooled sync arm and the single-threaded asyncio arm.
    """

    @staticmethod
    def cpu_seconds() -> float:
        r = resource.getrusage(resource.RUSAGE_SELF)
        return r.ru_utime + r.ru_stime

    @staticmethod
    def max_rss_mb() -> float:
        # Linux reports ru_maxrss in kilobytes (macOS uses bytes — this
        # harness is Linux-only, see the Dockerfile rationale).
        return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024


class SyncStreamer:
    """Drives Client.iter_bytes() across a thread pool.

    One shared Client so connection pooling is exercised the way real callers
    use it, rather than paying a fresh handshake per stream.
    """

    def __init__(self, url: str, iterations: int, concurrency: int):
        self.url = url
        self.concurrency = concurrency
        self.per_worker = max(1, iterations // concurrency)
        self.client = rqx.Client()
        self._total = 0
        self._lock = threading.Lock()

    def _worker(self) -> None:
        local = 0
        for _ in range(self.per_worker):
            with self.client.stream("GET", self.url) as resp:
                for chunk in resp.iter_bytes():
                    local += len(chunk)
        with self._lock:
            self._total += local

    def run(self) -> int:
        self._total = 0
        threads = [
            threading.Thread(target=self._worker) for _ in range(self.concurrency)
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        return self._total


class AsyncStreamer:
    """Drives AsyncClient.aiter_bytes() across asyncio tasks.

    This is the arm where PyBytesChunk actually does its work — the newtype
    converts at future-resolve time under the GIL pyo3 already holds.
    """

    def __init__(self, url: str, iterations: int, concurrency: int):
        self.url = url
        self.concurrency = concurrency
        self.per_worker = max(1, iterations // concurrency)

    async def _worker(self, client) -> int:
        local = 0
        for _ in range(self.per_worker):
            resp = await client.stream("GET", self.url)
            async for chunk in resp.aiter_bytes():
                local += len(chunk)
        return local

    async def _run(self) -> int:
        async with rqx.AsyncClient() as client:
            counts = await asyncio.gather(
                *[self._worker(client) for _ in range(self.concurrency)]
            )
        return sum(counts)

    def run(self) -> int:
        return asyncio.run(self._run())


@dataclass(frozen=True)
class Measurement:
    """One warmed, timed run, emitted as a single RunRecord."""

    GB = 1024**3

    arm: str
    mode: str
    label: str
    concurrency: int
    round: int

    def take(self, streamer) -> RunRecord:
        streamer.run()  # warmup: fills the pool, pages in the allocator arenas

        cpu_before = Usage.cpu_seconds()
        wall_before = time.perf_counter()
        total_bytes = streamer.run()
        wall = time.perf_counter() - wall_before
        cpu = Usage.cpu_seconds() - cpu_before

        gb = total_bytes / self.GB
        return RunRecord(
            arm=self.arm,
            mode=self.mode,
            payload=self.label,
            concurrency=self.concurrency,
            round=self.round,
            bytes=total_bytes,
            wall_s=round(wall, 6),
            cpu_s=round(cpu, 6),
            # The headline: CPU cost to move a fixed amount of data.
            cpu_s_per_gb=round(cpu / gb, 4) if gb else None,
            mb_s=round((total_bytes / (1024**2)) / wall, 2) if wall else None,
            max_rss_mb=round(Usage.max_rss_mb(), 2),
        )


@dataclass(frozen=True)
class Cli:
    measurement: Measurement
    streamer: object

    @classmethod
    def from_argv(cls) -> "Cli":
        p = argparse.ArgumentParser()
        p.add_argument("--arm", required=True, help="base | head")
        p.add_argument("--mode", required=True, choices=["sync", "async"])
        p.add_argument("--url", required=True)
        p.add_argument("--label", required=True, help="payload label, e.g. 1mb")
        p.add_argument("--iterations", type=int, required=True)
        p.add_argument("--concurrency", type=int, required=True)
        p.add_argument("--round", type=int, required=True)
        a = p.parse_args()

        streamer_cls = SyncStreamer if a.mode == "sync" else AsyncStreamer
        return cls(
            measurement=Measurement(
                a.arm, a.mode, a.label, a.concurrency, a.round
            ),
            streamer=streamer_cls(a.url, a.iterations, a.concurrency),
        )

    def run(self) -> None:
        print(self.measurement.take(self.streamer).to_json(), flush=True)


if __name__ == "__main__":
    Cli.from_argv().run()
