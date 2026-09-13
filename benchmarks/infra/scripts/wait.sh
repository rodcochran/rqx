#!/usr/bin/env bash
# Block until a run finishes, printing its phase once a minute. Exits with the run's exit code.
#
# Usage: scripts/wait.sh [RUN_ID]
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

resolve_run "${1:-}"
select_stack
read_outputs
while true; do
    sync_down
    run_finished && break
    if ! run_in_progress; then
        die "run $RUN_ID stopped without finishing; see scripts/status.sh"
    fi
    log "run $RUN_ID: $("$SCRIPTS_DIR/status.sh" "$RUN_ID" 2>/dev/null | head -1 | cut -d, -f2- | sed 's/^ //')"
    sleep 60
done
code="$(cat "$LOCAL_RESULTS/exit_code")"
log "run $RUN_ID finished with exit $code"
exit "$code"
