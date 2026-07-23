"""One measurement, as written to disk and read back."""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass


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
