"""Text tables.

Rows are dataclasses and columns name the field they display, so nothing
depends on positional ordering. Widths come from the content.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Column:
    field_name: str
    header: str
    align: str = ">"

    def value_of(self, row: object) -> str:
        return getattr(row, self.field_name)


@dataclass(frozen=True)
class Table:
    columns: list[Column]
    rows: list[object]
    GAP = 2

    def widths(self) -> list[int]:
        return [
            max([len(column.header)] + [len(column.value_of(row)) for row in self.rows])
            for column in self.columns
        ]

    def _line(self, values: list[str]) -> str:
        parts = [
            f"{value:{column.align}{width}}"
            for value, column, width in zip(values, self.columns, self.widths())
        ]
        return (" " * self.GAP).join(parts).rstrip()

    def render(self) -> str:
        header = self._line([column.header for column in self.columns])
        body = [
            self._line([column.value_of(row) for column in self.columns])
            for row in self.rows
        ]
        return "\n".join([header, "-" * len(header)] + body)
