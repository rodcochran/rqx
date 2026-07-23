"""Record schema shared by bench_stream (writes) and compare (reads).

One definition imported by both, so a field rename cannot desync them.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass


@dataclass(frozen=True, order=True)
class Config:
    """A benchmark configuration, independent of which build ran it."""

    mode: str
    payload: str
    concurrency: int

    def render(self) -> str:
        return f"{self.mode} {self.payload} c={self.concurrency}"


@dataclass(frozen=True)
class RunRecord:
    """One timed run of one build. Serialized as a line of raw.jsonl."""

    build: str
    mode: str
    payload: str
    concurrency: int
    round: int
    bytes: int
    wall_s: float
    cpu_s: float
    cpu_s_per_gb: float | None
    mb_s: float | None
    max_rss_mb: float

    @property
    def config(self) -> Config:
        return Config(
            mode=self.mode, payload=self.payload, concurrency=self.concurrency
        )

    @classmethod
    def from_json(cls, line: str) -> RunRecord:
        raw = json.loads(line)
        return cls(**{name: raw[name] for name in cls.__dataclass_fields__})

    def to_json(self) -> str:
        return json.dumps(asdict(self))
