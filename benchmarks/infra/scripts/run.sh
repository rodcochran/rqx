#!/usr/bin/env bash
# Build a ref on the client and start the benches there, detached from this session.
# The run id is printed and remembered for status.sh / collect.sh.
#
# Usage: scripts/run.sh [--ref REF] [--runs N]     (default: main, 5 runs per bench)
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

ref="main"
runs=5
while [[ $# -gt 0 ]]; do
    case "$1" in
        --ref) ref="$2"; shift 2 ;;
        --runs) runs="$2"; shift 2 ;;
        *) die "run: unknown flag $1" ;;
    esac
done

select_stack
read_outputs
wait_for_ssh "$CLIENT_IP" "client"
if run_in_progress; then
    die "a run is still in progress on the client; see scripts/status.sh"
fi
log "preparing the client for $ref (release build; ~4 min cold, ~1 min warm)..."
ssh_client "bash -s $SERVER_IP_PRIVATE $ref" < "$SCRIPTS_DIR/client-setup.sh"

RUN_ID="$(date -u +%Y%m%d-%H%M%S)"
ssh_client 'cat > run-benches.sh' < "$SCRIPTS_DIR/run-benches.sh"
# nohup so the run survives this laptop's session; exit_code marks the end. Only the nohup
# command is backgrounded and all three of its fds are redirected, so ssh returns at once.
ssh_client "mkdir -p results/$RUN_ID; RUNS_PER_BENCH=$runs nohup bash -c \
    'bash run-benches.sh $RUN_ID; echo \$? > results/$RUN_ID/exit_code' \
    > results/$RUN_ID/driver.log 2>&1 < /dev/null &"
mkdir -p "$RESULTS_ROOT"
echo "$RUN_ID" > "$RESULTS_ROOT/.current-run"
log "started run $RUN_ID ($ref, $runs runs per bench; ~95 min at 5)"
log "next: scripts/status.sh"
