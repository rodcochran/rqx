#!/usr/bin/env bash
# Drives the per-client b1 benches. Each (client × concurrency × run) is its
# own Python process — fresh event loop, fresh imports, fresh tokio runtime,
# no executor pollution from foreign clients. Results stream to stdout as
# one JSON object per line.
#
# Usage: bash benchmarks/run_b1.sh [--runs N] [--out results.jsonl]
set -euo pipefail

RUNS=5
OUT="b1_results.jsonl"
CONCURRENCIES=(10 50 100 500 1000)
CLIENTS=(rqx httpr httpx aiohttp)

while [[ $# -gt 0 ]]; do
    case "$1" in
        --runs) RUNS="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        *) echo "unknown flag: $1" >&2; exit 1 ;;
    esac
done

: > "$OUT"

for run in $(seq 1 "$RUNS"); do
    for c in "${CONCURRENCIES[@]}"; do
        for client in "${CLIENTS[@]}"; do
            # aiohttp's connector deadlocks at c=1000 under sustained load —
            # skip rather than hang the run.
            if [[ "$client" == "aiohttp" && "$c" == "1000" ]]; then
                echo "{\"client\":\"aiohttp\",\"concurrency\":1000,\"run\":$run,\"skipped\":\"connector_deadlock\"}" | tee -a "$OUT"
                continue
            fi
            echo "[run $run] $client c=$c" >&2
            python -u "benchmarks/b1_${client}.py" --c "$c" --run "$run" | tee -a "$OUT"
            # cool-down between measurements: lets TIME_WAIT clear, kernel TCP
            # buffers settle, server keepalive timers reset.
            sleep 5
        done
    done
done

echo "[done] results at $OUT" >&2
