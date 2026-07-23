"""One timed measurement: streams a config with rqx and prints a RunRecord.

Runs inside a build's virtualenv, so this is the only file that imports rqx.
"""

from __future__ import annotations

import argparse
import asyncio
import resource
import threading
import time
from dataclasses import dataclass

import rqx
from configs import Configs, RunConfig
from records import RunRecord

GB = 1024**3


class Usage:
    """Process-wide CPU and peak memory. RUSAGE_SELF covers every thread."""

    @staticmethod
    def cpu_seconds() -> float:
        usage = resource.getrusage(resource.RUSAGE_SELF)
        return usage.ru_utime + usage.ru_stime

    @staticmethod
    def peak_mb() -> float:
        # Linux reports ru_maxrss in kilobytes; this harness is Linux only.
        return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024


@dataclass(frozen=True)
class Streamer:
    """Streams the config's payload repeatedly and counts the bytes."""

    config: RunConfig
    url: str

    @property
    def per_worker(self) -> int:
        return max(1, self.config.iterations // self.config.concurrency)

    def _sync_worker(self, client, counts: list[int], slot: int) -> None:
        total = 0
        for _ in range(self.per_worker):
            with client.stream("GET", self.url) as response:
                for chunk in response.iter_bytes():
                    total += len(chunk)
        counts[slot] = total

    def _run_sync(self) -> int:
        # One shared client so connection pooling works as it does for callers.
        client = rqx.Client()
        counts = [0] * self.config.concurrency
        threads = [
            threading.Thread(target=self._sync_worker, args=(client, counts, slot))
            for slot in range(self.config.concurrency)
        ]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        return sum(counts)

    async def _async_worker(self, client) -> int:
        total = 0
        for _ in range(self.per_worker):
            response = await client.stream("GET", self.url)
            async for chunk in response.aiter_bytes():
                total += len(chunk)
        return total

    async def _gather(self) -> int:
        async with rqx.AsyncClient() as client:
            counts = await asyncio.gather(
                *[self._async_worker(client) for _ in range(self.config.concurrency)]
            )
        return sum(counts)

    def run(self) -> int:
        if self.config.mode == "sync":
            return self._run_sync()
        return asyncio.run(self._gather())


@dataclass(frozen=True)
class Measurement:
    """One warmed, timed run."""

    build: str
    config: RunConfig
    round: int
    url: str

    def take(self) -> RunRecord:
        streamer = Streamer(config=self.config, url=self.url)
        streamer.run()  # warmup: fills the pool, pages in the allocator arenas

        cpu_before = Usage.cpu_seconds()
        wall_before = time.perf_counter()
        total_bytes = streamer.run()
        wall = time.perf_counter() - wall_before
        cpu = Usage.cpu_seconds() - cpu_before

        gigabytes = total_bytes / GB
        return RunRecord(
            build=self.build,
            config=self.config.name,
            round=self.round,
            cpu_s_per_gb=round(cpu / gigabytes, 4) if gigabytes else 0.0,
            max_rss_mb=round(Usage.peak_mb(), 2),
            wall_s=round(wall, 6),
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="stream one config, print a record")
    parser.add_argument("--build", required=True)
    parser.add_argument("--config", required=True)
    parser.add_argument("--round", type=int, required=True)
    parser.add_argument("--base-url", required=True)
    args = parser.parse_args()

    config = Configs.select(args.config)[0]
    measurement = Measurement(
        build=args.build,
        config=config,
        round=args.round,
        url=f"{args.base_url}{config.path}",
    )
    print(measurement.take().to_json(), flush=True)
