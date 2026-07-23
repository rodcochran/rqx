# Streaming A/B — issue #108 / PR #139

Measures whether removing the per-chunk `Bytes -> Vec<u8> -> PyBytes` double
copy actually makes streaming cheaper, by building **two commits from source**
and running the identical benchmark against both.

| arm    | ref                                        | streaming path                          |
| ------ | ------------------------------------------ | --------------------------------------- |
| `base` | `5e3fe3e` (parent of the first PR commit)  | `bytes.to_vec()` on sync and async      |
| `head` | `6c83626` (PR tip)                         | `PyBytes::new` / `PyBytesChunk` newtype |

## Run

```bash
cd benchmarks/stream_ab
docker build -t rqx-stream-ab .
docker run --rm -v "$PWD/results:/results" rqx-stream-ab
```

Results land in `results/raw.jsonl` (one record per run) and
`results/summary.txt`. Override `BASE_REF`, `HEAD_REF`, or `ROUNDS` with `-e`.

## Design notes

**Why build both from source instead of diffing against the PyPI wheel.**
A released wheel contains every change since that release, not just this PR, so
the delta would be misattributed. It is also built by CI with its own profile —
comparing against a locally built wheel would measure build configuration as
much as code. Here both arms use one toolchain, one set of flags, one
container; the only variable is the commit.

**Why the benchmark script is not committed to either arm.** `base` predates
this file. The harness injects `bench_stream.py` into both venvs, so the
measurement code is byte-identical across arms.

**Why Linux, not the host.** The change removes a `malloc` per chunk, and
glibc's allocator behaves differently from macOS libmalloc. Running on the host
gives the right direction but a magnitude that does not transfer to the wheels
we ship.

**Why nginx runs inside the same container.** `benchmarks/nginx/nginx-host.conf`
documents that Docker Desktop's virtio networking caps out around 100 KB
payloads on macOS. Loopback within a single container never crosses the VM
boundary, so large bodies are not bottlenecked by the hypervisor.

**Why arms interleave within each round.** Running all of `base` then all of
`head` lets session-long drift — thermal, VM scheduling, page cache — bias
whichever arm ran second. Alternating spreads it across both.

## Reading the results

**CPU seconds per GB is the headline, not MB/s.** The change removes CPU and
allocator work. Even over loopback the workload is partly transfer-bound, so a
real CPU win can vanish inside wall-clock noise. If the two metrics disagree,
believe CPU time. Peak RSS is reported as a secondary signal — the eliminated
`Vec<u8>` should show up there too.

**`noise` means p >= 0.05** on a two-sided permutation test (20k resamples) of
the difference of medians. Raise `ROUNDS` if a cell you care about lands in
`noise` — unlike a range-based rule, this test gets *stronger* with more data.

An earlier version of this script judged significance by whether the arms'
observed ranges overlapped. That was wrong, and wrong in an instructive
direction: half-range is an extreme-value statistic that only grows as rounds
are added, so going from 5 to 10 rounds turned every verdict into `noise` while
the measured deltas barely moved. Do not reintroduce a range-based rule.

**Mind the multiple comparisons.** The table runs 3 metrics x 8 cells = 24
tests, so at p < 0.05 roughly one "significant" result is expected by chance
alone. Weight the cells whose p-values are an order of magnitude below the
threshold, and treat a lone marginal cell as a lead to investigate rather than
a finding.

## The result to be prepared for

The saving is roughly one `malloc` plus one `memcpy` per chunk — order 100 µs
per MB streamed. It is entirely possible this is **provable in principle but
invisible end-to-end**, especially in the 8 KB high-concurrency rows where
async machinery dominates. That is a legitimate outcome and worth reporting as
such. "Removes a per-chunk allocation and copy; not resolvable above noise in
end-to-end throughput" is a more honest note than rerunning until variance
produces a favorable number.
