"""Paired tables for a same-box A/B log written by infra/scripts/ab-client.sh."""

import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from statistics import median

LINE = re.compile(r"^(AB|CTL)\s+(\S.*?)\s+(\{.*\})$")


@dataclass
class Build:
    """One side of the A/B: its letter in the log, plus the SHA and branch when the run recorded them."""

    letter: str
    sha: str | None = None
    branch: str | None = None

    @property
    def label(self) -> str:
        if self.sha is None:
            return self.letter
        if self.branch is None:
            return f"{self.letter} {self.sha[:7]}"
        return f"{self.letter} {self.sha[:7]} ({self.branch})"

    @staticmethod
    def branch_at(sha: str) -> str | None:
        """The branch whose tip is exactly this SHA, if this checkout knows one."""
        try:
            names = subprocess.run(
                ["git", "for-each-ref", "--points-at", sha, "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"],
                capture_output=True,
                text=True,
                check=True,
            ).stdout.split()
        except (OSError, subprocess.CalledProcessError):
            return None
        local = (n.removeprefix("origin/") for n in names if n not in ("origin", "origin/HEAD"))
        return ", ".join(dict.fromkeys(local)) or None


@dataclass
class RunBuilds:
    """The builds a run's metadata.txt names (`sha_a=...`, `sha_b=...`)."""

    builds: list[Build]

    @classmethod
    def from_log(cls, log_path: str) -> "RunBuilds":
        metadata = Path(log_path).with_name("metadata.txt")
        if not metadata.exists():
            return cls(builds=[])
        builds = []
        for line in metadata.read_text().splitlines():
            key, _, sha = line.partition("=")
            if key.startswith("sha_") and sha:
                builds.append(Build(letter=key.removeprefix("sha_"), sha=sha, branch=Build.branch_at(sha)))
        return cls(builds=builds)

    def get(self, letter: str) -> Build:
        """A build the metadata doesn't name (the 09-22 follow-up arms) is just its letter."""
        return next((b for b in self.builds if b.letter == letter), Build(letter=letter))


@dataclass
class Sample:
    build: str
    concurrency: int
    pair: int
    position: int
    comparison: str
    rps: float
    peak_rss_mb: float


@dataclass
class Control:
    client: str
    concurrency: int
    at: str
    rps: float


@dataclass
class Pair:
    number: int
    baseline: Sample
    candidate: Sample

    @property
    def delta_pct(self) -> float:
        return (self.candidate.rps / self.baseline.rps - 1) * 100

    @property
    def candidate_first(self) -> bool:
        return self.candidate.position == 1


def parse(path: str) -> tuple[list[Sample], list[Control]]:
    samples: list[Sample] = []
    controls: list[Control] = []
    for line in open(path):
        m = LINE.match(line.strip())
        if not m:
            continue
        kind, fields, payload = m.groups()
        tags = dict(f.split("=", 1) for f in fields.split())
        result = json.loads(payload)
        if "skipped" in result:
            continue
        if kind == "AB":
            samples.append(
                Sample(
                    build=tags["build"],
                    concurrency=int(tags["c"]),
                    pair=int(tags["pair"]),
                    position=int(tags["pos"]),
                    comparison=tags.get("vs", "ba"),
                    rps=result["rps"],
                    peak_rss_mb=result["peak_rss_mb"],
                )
            )
        else:
            controls.append(
                Control(
                    client=tags["client"],
                    concurrency=int(tags["c"]),
                    at=tags["at"],
                    rps=result["rps"],
                )
            )
    return samples, controls


def pairs_at(samples: list[Sample], concurrency: int, comparison: str) -> list[Pair]:
    # A comparison tag is "<candidate><baseline>": "ba" is the split against v0.3.0.
    candidate, baseline = comparison[0], comparison[1]
    by_pair: dict[int, dict[str, Sample]] = {}
    for s in samples:
        if s.concurrency == concurrency and s.comparison == comparison:
            by_pair.setdefault(s.pair, {})[s.build] = s
    return [
        Pair(number=n, baseline=sides[baseline], candidate=sides[candidate])
        for n, sides in sorted(by_pair.items())
        if baseline in sides and candidate in sides
    ]


def report_pairs(pairs: list[Pair], concurrency: int, comparison: str, builds: RunBuilds) -> None:
    candidate, baseline = comparison[0], comparison[1]
    deltas = [p.delta_pct for p in pairs]
    wins = sum(d > 0 for d in deltas)
    print(f"\n== c={concurrency}, {candidate} vs {baseline}: {len(pairs)} pairs")
    sides = ((builds.get(baseline), [p.baseline for p in pairs]), (builds.get(candidate), [p.candidate for p in pairs]))
    width = max(len(build.label) for build, _ in sides)
    for build, side in sides:
        print(f"  {build.label:{width}s}  median {median(s.rps for s in side):9.0f} rps   rss {median(s.peak_rss_mb for s in side):6.1f} MB")
    print(f"  {candidate} vs {baseline}: median {median(deltas):+6.2f}%   {candidate} faster in {wins}/{len(pairs)} pairs   range {min(deltas):+.1f}% .. {max(deltas):+.1f}%")
    print("  per pair (first build listed first):")
    for p in pairs:
        if p.candidate_first:
            order = f"{candidate} {p.candidate.rps:8.0f}  {baseline} {p.baseline.rps:8.0f}"
        else:
            order = f"{baseline} {p.baseline.rps:8.0f}  {candidate} {p.candidate.rps:8.0f}"
        print(f"    {p.number:2d}  {order}   {p.delta_pct:+6.2f}%")


def report(samples: list[Sample], controls: list[Control], builds: RunBuilds) -> None:
    for c in sorted({s.concurrency for s in samples}):
        for comparison in ("ba", "ac", "bc", "da"):
            pairs = pairs_at(samples=samples, concurrency=c, comparison=comparison)
            if pairs:
                report_pairs(pairs=pairs, concurrency=c, comparison=comparison, builds=builds)
        ctl = [x for x in controls if x.concurrency == c]
        if ctl:
            print(f"  controls at c={c}:")
            for client in ("httpr", "aiohttp"):
                points = [f"{x.at} {x.rps:.0f}" for x in ctl if x.client == client]
                if points:
                    print(f"    {client:8s} {'  '.join(points)}")


if __name__ == "__main__":
    samples, controls = parse(sys.argv[1])
    if not samples:
        print("no samples yet")
    else:
        report(samples=samples, controls=controls, builds=RunBuilds.from_log(sys.argv[1]))
