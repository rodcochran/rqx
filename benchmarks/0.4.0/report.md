# rqx 0.4.0 — Performance Report

**rqx has the highest throughput and lowest median latency** of the four clients tested (rqx, httpr, aiohttp, httpx) at every concurrency, and the smallest memory footprint at c=10 and c=50. **aiohttp still wins tail-latency consistency** (p99/p50 of ~1.1× vs rqx's ~1.9–2.1×), and from c=100 up aiohttp, and from c=500 up httpr and httpx, use less memory than rqx.

**Throughput is up 7–14% against 0.3.0, 3.5–7 points more than the machines explain.** This instance pair is faster than 0.3.0's (httpr +3–7%, aiohttp +1–7% on identical versions), and rqx rose more than either at every concurrency. The extra is consistent with fat LTO, which this release turns on to recover the cross-crate inlining the crate split removed (see *Same-box A/B*). **Peak memory at c=500 and c=1000 is up 9–11 MB**, with every other client flat; it arrived with PR #196 or PR #202 and is not yet bisected (see *Memory*).

> *These benchmarks are basic and machine-dependent. They are intended as a rough comparison, not a definitive ranking. Results are specific to a 2-vCPU client on AWS c7i.large hitting a same-VPC nginx server over plaintext HTTP/1.1.*

Run: `20260927-231729` · raw logs in [`../results/aws-20260928-v040/`](../results/aws-20260928-v040/) · rqx at `a7c7f33` (`main` with PRs #196, #202 and #204; the binary reports 0.3.0 because the bump lands with the release)

## Throughput

![Throughput at concurrency=100](throughput.png)

At concurrency=100, rqx serves 21,190 RPS — 31% above httpr, 67% above aiohttp, and 36× above httpx.

Median RPS across 5 runs at each concurrency. Spread is min–max as a percentage of the median.

| Client  | c=10           | c=50           | c=100          | c=500          | c=1000         |
| ------- | -------------- | -------------- | -------------- | -------------- | -------------- |
| **rqx** | **17,997** ±4% | **21,056** ±3% | **21,190** ±4% | **21,137** ±6% | **20,549** ±3% |
| httpr   | 13,278 ±5%     | 15,682 ±3%     | 16,212 ±3%     | 16,744 ±3%     | 16,065 ±24%    |
| aiohttp | 12,614 ±16%    | 12,629 ±6%     | 12,705 ±5%     | 10,582 ±1%     | —              |
| httpx   | 1,012 ±7%      | 655 ±7%        | 592 ±6%        | 457 ±7%        | 395 ±17%       |

rqx's lead over the best other client (httpr at every concurrency): +36% at c=10, +34% at c=50, +31% at c=100, +26% at c=500, +28% at c=1000.

Spreads are tighter than 0.3.0's for rqx, and there is no session-long drift like 0.3.0's. rqx's first b1 run is its slowest at c=10, c=100 and c=500 (c=100: 20,510, then 21,383 · 21,174 · 21,190 · 21,225), and httpr's first is its slowest at c=100 and c=500: a 1–4% warm-up dip rather than a trend, which the medians step past. httpr's ±24% at c=1000 is one run (12,531 against 15,376–16,432 for the other four), and aiohttp's ±16% at c=10 is its first run (13,971 against 12,014–12,663).

### Versus 0.3.0

| c    | rqx 0.3.0 | rqx 0.4.0 | Δ rqx  | Δ httpr | Δ aiohttp | Δ httpx |
| ---: | --------: | --------: | -----: | ------: | --------: | ------: |
| 10   | 15,794    | 17,997    | +13.9% | +6.6%   | +3.9%     | +1.7%   |
| 50   | 19,749    | 21,056    | +6.6%  | +3.1%   | +1.4%     | −1.5%   |
| 100  | 19,648    | 21,190    | +7.8%  | +3.3%   | +4.2%     | +7.1%   |
| 500  | 19,021    | 21,137    | +11.1% | +4.6%   | +7.2%     | +8.6%   |
| 1000 | 18,697    | 20,549    | +9.9%  | +5.7%   | —         | 0.0%    |

The controls ran the same software as 0.3.0 (httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1, `rustc` 1.98.1, Python 3.12.3) with the same harness, so their movement is the instance pair: 1–7% faster. rqx beats httpr's movement by 3.5–7.3 points at every concurrency and aiohttp's by 3.6–10.0. httpx moves −1.5% to +8.6%, in the same band as the other controls; rqx's lead over it is 36× at c=100 and 52× at c=1000 (46× in 0.3.0, where httpx was the same 395 RPS).

### Same-box A/B

Two same-box A/Bs cover this release's code, each with 20 alternating pairs per concurrency and the build order flipped per pair, on paired `c7i.large` instances (`benchmarks/infra/scripts/ab.sh`):

- **The crate split and fat LTO (PR #202, run `ab-20260922-220938`, c=10).** Splitting rqx into a pure-Rust core and a binding crate cost 4.25% (split slower in 20/20 pairs): calls from the binding into core became calls across a crate boundary, which the compiler cannot inline without LTO. The same split with the old dependency versions was within 0.4% of the new ones, so the dependency bumps are not a factor. With fat LTO the split build was 4.06% faster than v0.3.0 (20/20 pairs) and its wheel 17% smaller.
- **The configuration refactor (PR #204, run `ab-20260927-171507`, `main` vs the branch).** −0.45% at c=10 (branch faster in 7/20 pairs), +0.31% at c=100 (11/20), −0.05% at c=500 (10/20), peak RSS within 0.7 MB. No detectable effect.

The release run's rqx lead over the controls, 3.5–7 points, is in line with the LTO result. Raw A/B logs are not archived with the release run; the conclusions are recorded here.

## Memory

![Memory at concurrency=100](memory.png)

Peak RSS in MB (median of 5 runs), measured via `resource.getrusage` in each client's own subprocess.

| Client  | c=10     | c=50     | c=100    | c=500    | c=1000   |
| ------- | -------- | -------- | -------- | -------- | -------- |
| **rqx** | **27.6** | **33.9** | 40.4     | 74.7     | 95.4     |
| aiohttp | 34.1     | 35.3     | **36.6** | **46.1** | —        |
| httpr   | 35.3     | 40.3     | 43.5     | 52.4     | **58.6** |
| httpx   | 34.2     | 39.2     | 43.7     | 56.5     | 66.0     |

**rqx's high-concurrency memory is up against 0.3.0: +8.9 MB (+14%) at c=500 and +10.6 MB (+13%) at c=1000.** Low concurrency is unchanged or better (c=10 −1.3 MB, c=50 ±0, c=100 +1.3 MB). Every other client is within 0.5 MB of 0.3.0 at every concurrency, so this is rqx, not the machine.

It is not this release's last change: PR #204's A/B shows 75.3 MB on `main` and 74.6 MB on the branch at c=500. The 22 September A/B already shows it: v0.3.0 at 66.3 MB against the split build at 77.3 MB at c=500, and 77.2 MB for the split with v0.3.0's dependency versions, so it came in with PR #196 (URL and QueryParams) or PR #202 (the crate split), not with dependency bumps. It scales with concurrency, roughly 10–20 KB per concurrent request, which points at something held per connection or per in-flight response rather than code size; PR #196's measured +80 bytes per live response is far too small to account for it. PR #196's own A/B reported no memory regression, but its memory analysis concentrated on low concurrency; if that holds at c=500, the crate split is the likelier source. Bisecting it is the next step: a same-box A/B of v0.3.0 against the PR #196 merge (`63863ed`) at c=500.

## Latency

![Median latency at concurrency=100](latency.png)

Per-request latency at c=100, 10,000 requests per client, median across 5 runs (`b2_latency.py`):

| Client  | p50         | p75         | p95         | p99         | p99.9        | max          |
| ------- | ----------- | ----------- | ----------- | ----------- | ------------ | ------------ |
| **rqx** | **4.39 ms** | **5.50 ms** | 7.65 ms     | 10.20 ms    | 14.43 ms     | 15.95 ms     |
| httpr   | 6.18 ms     | 7.59 ms     | 9.47 ms     | 10.92 ms    | **12.46 ms** | **14.00 ms** |
| aiohttp | 7.04 ms     | 7.10 ms     | **7.19 ms** | **7.65 ms** | 13.67 ms     | 14.44 ms     |
| httpx   | 107.19 ms   | 214.74 ms   | 557.30 ms   | 1,381.62 ms | 3,015.71 ms  | 4,686.34 ms  |

rqx's p50 improved 8.2% against 0.3.0 (4.78 → 4.39 ms) while httpr and aiohttp moved 1–2%; its p99 improved 4.7% (10.70 → 10.20 ms) against −2.8% to +0.4% for the controls. rqx leads through p75; aiohttp is ahead from p95 out. The tail is the delivery-path property tracked in https://github.com/rodcochran/rqx/issues/168, which this release does not touch.

## Tail consistency under load

p99/p50 ratio across concurrency (`b8_concurrency_sweep.py`, median of 10 samples per cell — 5 log files × 2 internal runs, zero recorded request failures):

| Client      | c=1      | c=10     | c=50     | c=100    |
| ----------- | -------- | -------- | -------- | -------- |
| **aiohttp** | 3.7×     | **1.2×** | **1.1×** | **1.1×** |
| rqx         | 2.1×     | 1.9×     | 2.1×     | 2.0×     |
| httpr       | 2.3×     | 1.8×     | 1.8×     | 1.8×     |
| httpx       | **1.4×** | 10.3×    | 5.7×     | 5.9×     |

Underlying medians at c=100: rqx p50 4.62 ms / p99 9.14 ms; aiohttp 7.11 / 7.66; httpr 6.26 / 10.96; httpx 117.84 / 689.65. rqx has the lowest absolute p50 from c=10 up and, against httpr, the lowest p99; aiohttp's p99 is lower than rqx's from c=10 up. rqx's ratios are within 0.1× of 0.3.0's (2.1 / 1.8 / 2.0 / 1.9 then). The c=1 column is round trip on this instance pair: every client's c=1 p50 is about 0.21 ms (0.29 ms on 0.3.0's pair), so it is not comparable across releases, and aiohttp's 3.7× there comes from a handful of slow requests over a 0.21 ms median.

## Stability

Zero aborts or crashes across 95 b1 cells (aiohttp c=1000 is skipped by design), 5 b2 runs, and 5 b8 runs, with zero recorded request failures. Fifth consecutive clean run since the runtime lifecycle fix in 0.1.4.

## Test setup

- **Client:** AWS `c7i.large` (2 vCPU, 4 GB), Ubuntu 24.04, kernel `7.0.0-1013-aws`, Python 3.12.3, `rustc 1.98.1`. rqx built from source at `a7c7f33` in release mode, which since PR #202 means fat LTO and one codegen unit.
- **Server:** AWS `c7i.large` in the same subnet running nginx (`benchmarks/nginx/`) serving a static 1.4 KB JSON body over plaintext HTTP/1.1 with keep-alive.
- **Comparison clients:** httpr 0.7.2, aiohttp 3.14.3, httpx 0.28.1, installed unpinned from PyPI at setup time and recorded in `metadata.txt` — the same versions as 0.2.0 and 0.3.0.
- **Harness:** `benchmarks/infra/scripts/run-benches.sh` — b1 (`run_b1.sh`, each (client, concurrency, run) in its own Python subprocess, 3 s warmup then 15 s measure on the same client), b2 (`b2_latency.py`), b8 (`b8_concurrency_sweep.py`), 5 runs each. Unchanged from 0.3.0.

## Limitations

- **A different instance pair than 0.3.0's,** 1–7% faster by the controls. The *Versus 0.3.0* table reads rqx against those controls, not in absolute terms.
- **The memory increase is not yet attributed** to PR #196 or PR #202; see *Memory*.
- **The throughput chart's httpx footnote is out of date.** It says httpx is bound by its default connection pool; the benches set 1,500 connections for every client, and the cause is httpcore re-scanning its pool on every request (see the [0.2.0 report](../0.2.0/report.md#limitations) and https://github.com/rodcochran/rqx/issues/188). The footnote is hardcoded in `plot_bench.py`.
- **aiohttp at c=1000 is skipped** because its connector deadlocks under the harness at that concurrency; that is a harness interaction, not a verdict on aiohttp.
- **Same-VPC RTT is sub-millisecond,** so every number here is client-CPU-bound. Over a real network the throughput gaps shrink and the latency gaps are dominated by the wire.

## Reproducing this

```bash
just benchmarks::setup <aws-profile>        # once
just benchmarks::release 0.4.0 v0.4.0       # ~95 min; results in benchmarks/results/aws-<date>-v040/, charts in benchmarks/0.4.0/
```

See [`../infra/README.md`](../infra/README.md) for the per-step commands and [`../README.md`](../README.md) for what each bench measures.
