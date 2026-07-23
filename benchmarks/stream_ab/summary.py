"""The shape of the JSON file. Data only, no behavior."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class RunInfo:
    base_ref: str
    head_ref: str
    rounds: int
    toolchain: str


@dataclass(frozen=True)
class About:
    question: str
    headline_metric: str
    method: str


@dataclass(frozen=True)
class BuildSummary:
    median: float
    lowest: float
    highest: float
    rounds: int


@dataclass(frozen=True)
class Answer:
    headline: str
    trustworthy: bool
    trust_detail: str
    rounds: int


@dataclass(frozen=True)
class SavingSummary:
    saving: float | None
    microseconds_per_mb: float | None
    based_on_configs: int


@dataclass(frozen=True)
class MetricSummary:
    verdict: str
    change_pct: float
    change: float
    chance_it_is_luck: float
    base: BuildSummary
    head: BuildSummary


@dataclass(frozen=True)
class ConfigSummary:
    config: str
    metrics: dict[str, MetricSummary]


@dataclass(frozen=True)
class ReportSummary:
    run: RunInfo
    about: About
    answer: Answer
    saving_check: SavingSummary
    configs: list[ConfigSummary]
