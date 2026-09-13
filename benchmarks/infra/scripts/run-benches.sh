#!/usr/bin/env bash
# Runs on the bench client. Executes b1, b2, b8 N times each and captures all
# output under ~/results/<run-id>/. The orchestrator scp's that directory down.
#
# Usage (via the orchestrator): run-benches.sh <RUN_ID>
set -euo pipefail

RUN_ID="${1:?usage: run-benches.sh <RUN_ID>}"
RUNS_PER_BENCH="${RUNS_PER_BENCH:-5}"

RESULTS_DIR="$HOME/results/$RUN_ID"
mkdir -p "$RESULTS_DIR"

cd "$HOME/rqx"
# shellcheck disable=SC1091
. .venv/bin/activate
# rustc/cargo aren't in PATH by default in a non-login SSH shell.
# shellcheck disable=SC1091
. "$HOME/.cargo/env" 2>/dev/null || true

# Record what we ran for later analysis.
{
    echo "run_id: $RUN_ID"
    echo "timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host: $(hostname)"
    echo "kernel: $(uname -r)"
    echo "rqx_commit: $(cd "$HOME/rqx" && git rev-parse HEAD)"
    echo "rqx_branch: $(cd "$HOME/rqx" && git rev-parse --abbrev-ref HEAD)"
    echo "rqx_version: $(grep '^version' "$HOME/rqx/Cargo.toml" | head -1 | cut -d'"' -f2)"
    # What the venv actually imports; should match rqx_version.
    echo "rqx_installed: $(python -c 'import importlib.metadata as m; print(m.version("rqx"))')"
    echo "python_version: $(python --version)"
    echo "rustc_version: $(rustc --version 2>/dev/null || echo 'rustc not on PATH')"
    echo "runs_per_bench: $RUNS_PER_BENCH"
    # Comparison clients are installed unpinned; record what this run measured against.
    echo "comparators: $(python -c 'import importlib.metadata as m; print(", ".join(f"{p} {m.version(p)}" for p in ["httpr", "aiohttp", "httpx"]))')"
} > "$RESULTS_DIR/metadata.txt"

run_one() {
    local bench="$1"
    local script="benchmarks/${bench}.py"
    echo "[bench] $bench ($RUNS_PER_BENCH runs)"
    for i in $(seq 1 "$RUNS_PER_BENCH"); do
        echo "[bench]   run $i/$RUNS_PER_BENCH"
        # `-u` forces unbuffered stdout/stderr so progress shows in tee + log
        # in real time. Without it, Python block-buffers when stdout is a
        # pipe and nothing reaches the file until the buffer fills.
        python -u "$script" 2>&1 | tee "$RESULTS_DIR/${bench}-run${i}.log"
    done
}

# Run benches sequentially — running them in parallel would have one bench's
# load skew the other's measurements.
#
# b1 has its own driver (run_b1.sh) that runs each client in its own Python
# process — avoiding executor pollution and per-process tokio contention.
echo "[bench] b1 ($RUNS_PER_BENCH runs per client/concurrency)"
bash benchmarks/run_b1.sh --runs "$RUNS_PER_BENCH" --out "$RESULTS_DIR/b1_results.jsonl" \
    2>&1 | tee "$RESULTS_DIR/b1.log"

run_one b2_latency
run_one b8_concurrency_sweep

echo "[bench] done. results at $RESULTS_DIR"
