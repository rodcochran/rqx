#!/usr/bin/env bash
# Full benchmark sweep with between-bench state reset.
#
# What this does:
#   - Rebuilds reqx in release mode
#   - Starts the local delay server on :8081 (for b7)
#   - Verifies both targets are reachable
#   - Runs b4, b7, b8, b1 in order (short → long)
#   - Between each bench: restarts nginx and sleeps to let TCP TIME_WAIT drain
#   - Captures per-bench output under /tmp/reqx_bench_<timestamp>/
#
# Why the between-bench dance:
#   - Localhost under sustained high-concurrency HTTP generates a lot of
#     TIME_WAIT state. A fresh bench starting with that state produces
#     mega-outliers in the tail (we've seen 4-second max values). Restarting
#     nginx + sleeping 10s clears most of it.
#   - Benches are ordered short→long so if something goes wrong early we
#     catch it before burning 12 min on b1.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# shellcheck disable=SC1091
source venv/bin/activate

OUT_DIR="/tmp/reqx_bench_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUT_DIR"
echo "=== Output dir: $OUT_DIR ==="

echo "=== Rebuilding reqx in release mode ==="
maturin develop --release > "$OUT_DIR/build.log" 2>&1
tail -3 "$OUT_DIR/build.log"

echo "=== Starting delay_server on :8081 ==="
python benchmarks/delay_server.py > "$OUT_DIR/delay_server.log" 2>&1 &
DELAY_PID=$!
sleep 2

cleanup() {
    if kill -0 "$DELAY_PID" 2>/dev/null; then
        kill "$DELAY_PID" 2>/dev/null || true
        echo "Stopped delay server (pid $DELAY_PID)"
    fi
}
trap cleanup EXIT

echo "=== Sanity checking targets ==="
curl -sf http://localhost:8080/json > /dev/null || {
    echo "ERROR: nginx not reachable on :8080 — bring up benchmarks/docker-compose.yaml first"
    exit 1
}
curl -sf http://localhost:8081/json > /dev/null || {
    echo "ERROR: delay_server not reachable on :8081"
    exit 1
}
echo "    nginx    :8080  OK"
echo "    delay    :8081  OK"

reset_state() {
    echo "--- Restarting nginx and draining TCP state (10s) ---"
    (cd benchmarks && docker compose restart nginx) > /dev/null
    # Wait for nginx to come back up
    for _ in $(seq 1 30); do
        if curl -sf http://localhost:8080/json > /dev/null; then
            break
        fi
        sleep 0.5
    done
    sleep 10
    echo "--- nginx ready ---"
}

run_bench() {
    local name="$1"
    shift
    echo ""
    echo "================================================================"
    echo "  BENCH: $name  |  $(date)"
    echo "================================================================"
    reset_state
    "$@" 2>&1 | tee "$OUT_DIR/${name}.log"
}

# Order: short → long. If something breaks we catch it early.
run_bench "b4_memory" \
    python benchmarks/b4_memory.py

run_bench "b7_network_latency" \
    python benchmarks/b7_network_latency.py

run_bench "b8_concurrency_sweep" \
    python benchmarks/b8_concurrency_sweep.py --runs 3 --json "$OUT_DIR/b8.json"

run_bench "b1" \
    bash benchmarks/run_b1.sh --runs 5 --out "$OUT_DIR/b1_results.jsonl"

echo ""
echo "================================================================"
echo "  DONE  |  $(date)"
echo "================================================================"
echo "Results:"
ls -la "$OUT_DIR"
