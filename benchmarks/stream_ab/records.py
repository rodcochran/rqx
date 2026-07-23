"""What each test item is, and what one measurement of it looks like.

Shared by every process in the harness: the runner picks configs from here, the
measurement script writes records, the analysis reads them back.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass


@dataclass(frozen=True)
class RunConfig:
    """One test item: how to stream, what to stream, and how many at once."""

    mode: str  # sync | async
    payload: str  # label, matches the nginx path
    iterations: int
    concurrency: int

    @property
    def name(self) -> str:
        return f"{self.mode} {self.payload} c={self.concurrency}"

    @property
    def path(self) -> str:
        return f"/{self.payload}"

    def matches(self, text: str) -> bool:
        return text in (self.name, f"{self.mode} {self.payload} {self.concurrency}")


class Configs:
    """Large payloads are where the saving should show. The 8kb items are the
    no-regression check: too small for the effect, so they should not move."""

    ALL = [
        RunConfig(mode="async", payload="1mb", iterations=120, concurrency=1),
        RunConfig(mode="async", payload="1mb", iterations=240, concurrency=8),
        RunConfig(mode="async", payload="10mb", iterations=24, concurrency=1),
        RunConfig(mode="sync", payload="1mb", iterations=120, concurrency=1),
        RunConfig(mode="sync", payload="1mb", iterations=240, concurrency=8),
        RunConfig(mode="sync", payload="10mb", iterations=24, concurrency=1),
        RunConfig(mode="async", payload="8kb", iterations=8000, concurrency=64),
        RunConfig(mode="sync", payload="8kb", iterations=8000, concurrency=64),
    ]

    @classmethod
    def select(cls, only: str | None) -> list[RunConfig]:
        if not only:
            return cls.ALL
        chosen = [config for config in cls.ALL if config.matches(only)]
        if not chosen:
            names = ", ".join(config.name for config in cls.ALL)
            raise SystemExit(f"no config matches {only!r}. Available: {names}")
        return chosen


@dataclass(frozen=True)
class RunRecord:
    """One timed run of one build, written as a line of JSON."""

    build: str  # base | head
    config: str  # RunConfig.name
    round: int
    cpu_s_per_gb: float
    max_rss_mb: float
    wall_s: float

    @classmethod
    def from_json(cls, line: str) -> RunRecord:
        raw = json.loads(line)
        return cls(**{name: raw[name] for name in cls.__dataclass_fields__})

    def to_json(self) -> str:
        return json.dumps(asdict(self))
