"""The record schema shared by the producer (bench_stream) and the consumer
(compare).

One definition, imported by both, so a field rename cannot silently desync the
two halves of the harness. Both run from /harness inside the container, so the
script directory is already on sys.path.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass


@dataclass(frozen=True, order=True)
class Cell:
    """A benchmark configuration, independent of which arm ran it."""

    mode: str
    payload: str
    concurrency: int

    def render(self) -> str:
        return f"{self.mode} {self.payload} c={self.concurrency}"


@dataclass(frozen=True)
class RunRecord:
    """One timed run of one arm. Serialized as a line of raw.jsonl."""

    arm: str
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
    def cell(self) -> Cell:
        return Cell(self.mode, self.payload, self.concurrency)

    @classmethod
    def from_json(cls, line: str) -> RunRecord:
        raw = json.loads(line)
        return cls(**{f: raw.get(f) for f in cls.__dataclass_fields__})

    def to_json(self) -> str:
        return json.dumps(asdict(self))
