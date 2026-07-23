"""The test items: what gets streamed, how, and how many at once."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class RunConfig:
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
