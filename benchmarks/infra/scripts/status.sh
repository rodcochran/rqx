#!/usr/bin/env bash
# Where a run is: state, current bench, and the b1 medians so far next to the last archive.
# Copies the run's directory down from the client each time.
#
# Usage: scripts/status.sh [RUN_ID]
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

resolve_run "${1:-}"
select_stack
read_outputs
sync_down

if run_finished; then
    code="$(cat "$LOCAL_RESULTS/exit_code")"
    [[ "$code" == "0" ]] && state="finished" || state="failed (exit $code)"
elif run_in_progress; then
    state="running"
else
    state="stopped without finishing"
fi

driver="$LOCAL_RESULTS/driver.log"
[[ -f "$driver" ]] || driver="$LOCAL_RESULTS/b1.log"
if [[ ! -f "$driver" ]]; then
    phase="not started"
elif grep -q '^\[bench\] done' "$driver"; then
    phase="all benches done"
else
    bench="$(grep -oE '^\[bench\] (b1|b2_latency|b8_concurrency_sweep)' "$driver" | tail -1 | cut -d' ' -f2)"
    run="$(grep -oE '^\[(run [0-9]+\]|bench\]   run [0-9]+/[0-9]+)' "$driver" | tail -1 | sed 's/.*run /run /; s/\]//')"
    phase="${bench:-setup} ${run:-}"
fi

echo "run $RUN_ID: $state, $phase"
echo "local copy: $LOCAL_RESULTS"
if [[ -f "$LOCAL_RESULTS/b1_results.jsonl" ]]; then
    echo
    "$(repo_python)" "$REPO_DIR/benchmarks/compare_b1.py" "$LOCAL_RESULTS/b1_results.jsonl"
fi
if [[ "$state" != "running" && "$state" != "finished" && -f "$LOCAL_RESULTS/driver.log" ]]; then
    echo
    echo "last lines of driver.log:"
    tail -5 "$LOCAL_RESULTS/driver.log"
fi
