# Streaming A/B — issue #108 / PR #139

## What this answers

PR #139 removes one copy of every streamed byte. Chunks used to be copied
twice — into a `Vec<u8>`, then into a Python `bytes` — and now they are copied
once.

**Does that make streaming measurably cheaper?**

## Run it

```bash
just bench-stream 10
```

That is the whole thing. It builds both commits from source in a Linux
container, runs them head to head, and prints a verdict.

First run takes 10–20 minutes (rustup plus two release builds). Later runs
reuse the Docker layer cache.

## Read it

The output starts with the answer:

```
VERDICT  head is faster

  CPU   -0.032 s/GB (~31 us/MB) in 3 of 8 configs
        one constant explains the rest, including the configs
        where the effect is too small to measure
  RSS   -15% at async 1mb c=8 (4 of 8 configs improved)

  Trustworthy: yes — 10 paired rounds, arms alternated, drift under 10%
```

**Check the `Trustworthy` line first.** If it says `NO`, it tells you what to
fix — usually "use more rounds" or "quiet the box and rerun". Everything above
it is meaningless until that line says yes.

**Then read `CPU`.** That is the headline: how many CPU-seconds are saved per
GB streamed. It is an absolute number rather than a percentage on purpose —
see below.

Add `--detail` (or open `results/summary-sweep.txt`, which always has it) for
per-config tables, p-values, and the mechanism check.

## Running one config

If a single configuration looks odd, drill into it rather than repeating the
whole sweep — statistical power comes from rounds, and rounds are cheapest
spent on one config:

```bash
just bench-stream-cell "async 1mb 8" 40
```

Results are written under their own `raw-cell-*` / `summary-cell-*` names, so
a drill-down never clobbers the sweep you are comparing it against.

## Why the headline is absolute, not a percentage

Removing one copy costs a fixed number of CPU-seconds per GB, no matter the
payload size or concurrency. So the same absolute saving should appear in every
config, and the percentage should differ only because the baselines differ.

That makes the claim falsifiable: one constant has to explain all eight
configs, including correctly predicting the ones where the effect is too small
to see. The `MECHANISM CHECK` section in `--detail` shows implied vs observed
per config. A cell that disagrees is a harness problem, not a discovery.

Throughput is deliberately not analyzed. At this effect size it was noise and
produced more confusion than signal. Raw records still contain `mb_s` if you
want to look.

---

## Methodology notes

Skip this unless you are changing the harness.

**Both arms are built from source.** Not diffed against a PyPI wheel: a
released wheel contains every change since the release, and was built by CI
with its own profile, so that comparison would measure build configuration as
much as code. Here one toolchain, one set of flags, one container.

**The benchmark script is not committed to either arm.** `base` predates it.
The harness injects `bench_stream.py` and `records.py` into both venvs so the
measurement code is byte-identical across arms.

**Linux, not the host.** The change removes a `malloc`, and glibc's allocator
behaves differently from macOS libmalloc. The host gives the right direction
but a magnitude that does not transfer to the wheels we ship.

**nginx runs inside the same container.** `benchmarks/nginx/nginx-host.conf`
documents that Docker Desktop's virtio networking caps out around 100 KB
payloads on macOS. Loopback within one container never crosses the VM boundary.

**Arms interleave within a round, and the order alternates.** Running all of
`base` then all of `head` lets session drift bias whichever went second.
Interleaving fixes that, but running `base` first in *every* round is itself an
uncontrolled order effect — and since the analysis is paired within a round,
pairing bakes it in rather than cancelling it. The order flips on even rounds.
**Use an even `ROUNDS`** so the two orders stay balanced.

**Analysis is paired, tested by sign-flip permutation.** Differences are taken
within a round, so machine drift cancels. Significance is a two-sided sign-flip
test on those paired differences (20k resamples, alpha 0.05).

Two earlier rules failed and should not be reintroduced:

1. *Range overlap.* Half-range is an extreme-value statistic that only grows as
   rounds are added, so going 5 → 10 rounds turned every verdict into noise
   while the deltas barely moved. A rule that weakens as evidence accumulates
   is backwards.
2. *Unpaired permutation.* Correct for i.i.d. samples, but these are not: over
   one 15-round session the base arm's CPU/GB rose ~60% and its throughput
   halved. Interleaving cancels bias between arms, but each arm's samples still
   span the drift, so the test loses nearly all power.

**Mind multiple comparisons.** Two metrics across eight configs is 16 tests, so
at p < 0.05 roughly one "significant" result is expected by chance. Weight the
configs whose p-values are well below the threshold, and treat a lone marginal
cell as a lead rather than a finding. The mechanism check is the stronger
evidence, because it is a prediction rather than a search.
