# Contributing

## Finding ways to help

This project started as an experiment, with the goal of learning. It evolved quickly into a genuine contender for high-concurrency HTTP handling in Python.

The goal is to be maintain feature parity with [httpx](https://github.com/encode/httpx), but with the performance of async Rust. This basically creates 2 natural places for improvements: feature parity and performance, with correctness as table stakes.

There are several tags we use to make that simple: `httpx-feature-parity` and then your run-of-the-mill tags like `bug`, `enhancement`, etc.

## Use of AI

This project started out as learning project. I used AI to help me learn some of the basics but wrote the large majority by hand... double-edged sword. So as we advance, I encourage the use of AI for breadth and speed. It's obvious when someone is using AI to write the code for them, let's try to avoid that so that when an issue comes up, someone can speak to it without relying on their AI to go dig for them.

### Good use cases for AI

- Getting it to run and analyze the benchmarks after a change.
- Doing a breadth-first search for alternatives.
- Experimenting with a variety of things in parallel.

### Bad use cases for AI

- Writing tests that don't mean anything.
- Refactoring large chunks of code.
- Assessing benchmarks.
- Writing up bug reports that you haven't experienced first hand.

## Setup

### Prerequisites

Rust is the only requirement for local dev, however would strongly encourage the use of `just`, `uv`, and `rustup` with `clippy`.

- [Rust toolchain](https://rustup.rs/) (with `clippy`)
- Python 3.9+
- [uv](https://github.com/astral-sh/uv) — venv + dependency management
- [just](https://github.com/casey/just) — task runner (every doc references it)

### First-time setup

```bash
just setup    # uv venv + dev deps + maturin develop
just test     # full test suite (parallel via xdist)
```

`just setup` chains `uv venv`, `uv pip install -e ".[dev]"`, `uv lock`, and `maturin develop`. Skip `just` and you can run those steps directly.

## Project layout

```
src/                     Rust core — pyo3 classes, transport, retry, etc.
python/rqx/              Python wrapper — re-exports + module-level functions
python/rqx/_types.pyi    Type stubs for the compiled extension
tests/                   pytest suite (sync + async + MTLS + streaming)
benchmarks/              Performance scripts
docs/                    Project spec, report, benchmark output
```

## Submitting changes

- Branch from `main`, one PR per logical change.
- PR description: short Summary, `Closes #N`, then a Testing section in prose describing what was verified. No checklist-style Test plan — say what was actually run and what it proved.
- Run `just test` locally before opening the PR. CI runs the same suite on Linux.

## Reporting bugs

Open an issue with:

- A minimal reproducible snippet (Python code + the `rqx` version)
- What you expected to happen
- What actually happened (status codes, stack trace, whatever's relevant)
- Environment (OS, Python version)

A small repro is worth ten paragraphs of context.
