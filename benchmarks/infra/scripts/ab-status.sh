#!/usr/bin/env bash
# Copy an A/B run down and print the paired tables so far.
#
# Usage: scripts/ab-status.sh [RUN_ID]
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

resolve_run "${1:-}"
select_stack
read_outputs
sync_down

if run_finished; then
    code="$(cat "$LOCAL_RESULTS/exit_code")"
    [[ "$code" == "0" ]] && state="finished" || state="failed (exit $code)"
elif ssh_client "pgrep -f '[a]b-client\\.sh' >/dev/null"; then
    state="running"
else
    state="stopped without finishing"
fi
echo "run $RUN_ID: $state ($(tail -1 "$LOCAL_RESULTS/driver.log" 2>/dev/null))"
echo "local copy: $LOCAL_RESULTS"
[[ -f "$LOCAL_RESULTS/ab.log" ]] && "$(repo_python)" "$REPO_DIR/benchmarks/ab_report.py" "$LOCAL_RESULTS/ab.log"
