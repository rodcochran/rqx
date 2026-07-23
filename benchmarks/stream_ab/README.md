# Streaming benchmark — issue #108 / PR #139

## What this answers

PR #139 removes one copy of every streamed byte. Chunks used to be copied twice
— into a `Vec<u8>`, then into a Python `bytes` — and now they are copied once.

**Does that make streaming measurably cheaper?**

## Run it

```bash
just bench-stream 10
```

That is the whole thing. It builds both commits from source in a Linux
container, runs them head to head, and prints an answer.

The first run takes 10–20 minutes because it installs Rust and does two release
builds. Later runs reuse the Docker cache.

## Read it

```
ANSWER  head is faster

  CPU     -0.032 s/GB (about 32 microseconds per MB) in 3 of 8 configs
          the same saving explains the rest, including the
          configs where it is too small to see
  Memory  -15% less at async 1mb c=8 (4 of 8 configs improved)

  Can you trust this? yes — 10 rounds, builds alternated, machine stayed steady
```

**Read the last line first.** If it starts with `NO`, it says what to fix —
usually more rounds, or close other apps and rerun. Nothing above it means
anything until that line says yes.

Then read `CPU`. That is the headline: how many CPU-seconds are saved for every
GB streamed.

The per-config tables print underneath. They are also saved to
`results/answer-sweep.txt`, with the same numbers as JSON alongside.

## Terms

- **base** and **head** are the two builds being compared: the commit before
  the change, and the commit with it.
- **config** is one row of the test matrix — sync or async, payload size, and
  how many streams run at once. `async 1mb c=8` is eight concurrent async
  streams of a 1 MB body.
- **round** is one pass through every config, running both builds back to back.
- **chance** is how often random variation alone would produce a difference
  this big. Under 5% and we call it real; otherwise `too small to tell`.

## Running one config

If a single config looks odd, drill into it rather than repeating the whole
sweep. Confidence comes from rounds, and rounds are cheapest on one config:

```bash
just bench-stream 40 "async 1mb 8"
```

Results get their own file names, so a drill-down never overwrites the sweep you
are comparing it against.

## Why the headline is seconds, not a percentage

Removing one copy costs the same amount of CPU per GB no matter how big the
payload is or how many streams run at once. So the same saving in seconds
should turn up in every config, and only the percentage should differ, because
the percentage depends on how much CPU that config used to begin with.

That makes the claim checkable: one number has to explain all the configs,
including correctly predicting the ones where the saving is too small to
notice. The `expected` column shows what the single saving predicts for each
config, next to what was measured. A config that disagrees means the benchmark
is measuring something else.

Throughput is not analyzed. At this size of difference it was pure noise.

---

## Notes for changing the harness

**Both builds come from source.** Not compared against a PyPI wheel: a released
wheel contains every change since that release, and CI built it with its own
settings, so that comparison would measure the build setup as much as the code.

**The benchmark script is not committed to either build.** The base commit
predates it, so the harness copies the measurement code into both
environments. Same code either side, only the wheel differs.

**Linux, not macOS.** The change removes a `malloc`, and Linux and macOS
allocate memory differently. Running on the host gives the right direction but a
size that does not carry over to the wheels we ship.

**nginx runs in the same container.** `benchmarks/nginx/nginx-host.conf`
explains that Docker Desktop's networking on macOS falls over above about
100 KB. Traffic inside one container never leaves it.

**Builds alternate order every round.** Running base first every time would hand
a systematic advantage to whichever went second — warm caches, a CPU already at
full speed — and since builds are compared within a round, that advantage would
land in the result. **Keep `ROUNDS` even** so the two orders balance out.

**Comparisons happen within a round.** Machines speed up and slow down over a
long run. Two builds that ran seconds apart saw the same conditions; averages
taken an hour apart did not. Comparing across rounds instead of within them
loses almost all ability to detect a difference this small.

**Two metrics across eight configs is sixteen comparisons.** At a 5% threshold,
roughly one will look real by chance. Trust the configs whose `chance` is well
under the threshold, and treat a single borderline one as something to
investigate rather than a result.

## The files

| file | what it is |
| ---- | ---------- |
| `configs.py` | the test items: mode, payload, iterations, concurrency |
| `records.py` | one measurement, written and read as JSON |
| `measurement.py` | streams one config once and prints a record |
| `experiment.py` | runs every config against both builds |
| `comparison.py` | compares one config between the builds |
| `report.py` | the printed answer and the JSON beside it |
| `tables.py` | text table layout |
| `entrypoint.sh` | clones both commits, builds two venvs, hands off |
