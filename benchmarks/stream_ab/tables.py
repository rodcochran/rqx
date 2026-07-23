"""Text output: columns, tables, and the sections that hold them.

Rows are dataclasses and columns name the field they display, so nothing
depends on positional ordering. Widths come from the content.
"""

from __future__ import annotations

from dataclasses import dataclass, field


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
    indent: str = ""
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
        return self.indent + (" " * self.GAP).join(parts).rstrip()

    def render(self) -> str:
        header = self._line([column.header for column in self.columns])
        rule = self.indent + "-" * (len(header) - len(self.indent))
        body = [
            self._line([column.value_of(row) for column in self.columns])
            for row in self.rows
        ]
        return "\n".join([header, rule] + body)


@dataclass(frozen=True)
class Section:
    """A heading, optional prose, and an optional table.

    Every part of the report is one of these, so the spacing and heading style
    are decided once here rather than in each part.
    """

    heading: str
    lines: list[str] = field(default_factory=list)
    table: Table | None = None

    def render(self) -> str:
        parts = [self.heading] + self.lines
        if self.table is not None:
            if self.lines:
                parts.append("")
            parts.append(self.table.render())
        return "\n".join(parts) + "\n"
