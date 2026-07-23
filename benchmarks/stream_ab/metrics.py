"""The quantities being compared, and how each one is displayed."""

from __future__ import annotations

from dataclasses import dataclass

from records import RunRecord


@dataclass(frozen=True)
class Metric:
    key: str
    label: str
    unit: str
    value_format: str
    change_format: str  # changes are much smaller than the values, so finer

    def value_of(self, record: RunRecord) -> float | None:
        return getattr(record, self.key)


class Metrics:
    """The metrics this benchmark reports, in the order they are shown.

    Throughput is left out on purpose: at this size of difference it was pure
    noise. Raw records still carry mb_s for anyone who wants to look.
    """

    CPU = Metric(
        key="cpu_s_per_gb",
        label="CPU seconds per GB",
        unit="s/GB",
        value_format="{:.3f}",
        change_format="{:+.4f}",
    )
    MEMORY = Metric(
        key="max_rss_mb",
        label="Peak memory",
        unit="MB",
        value_format="{:.1f}",
        change_format="{:+.2f}",
    )
    ALL = [CPU, MEMORY]
