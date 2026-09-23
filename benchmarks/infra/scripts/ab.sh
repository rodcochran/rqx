#!/usr/bin/env bash
# Same-box A/B of two refs: both built on the client, samples alternated per pair.
# Starts detached; the run id is printed and remembered for ab-status.sh.
#
# Usage: scripts/ab.sh --a SHA --b SHA [--pairs N] [--c "10 100 500"]
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

sha_a=""; sha_b=""; pairs=20; concurrencies="10 100 500"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --a) sha_a="$2"; shift 2 ;;
        --b) sha_b="$2"; shift 2 ;;
        --pairs) pairs="$2"; shift 2 ;;
        --c) concurrencies="$2"; shift 2 ;;
        *) die "ab: unknown flag $1" ;;
    esac
done
[[ ${#sha_a} -eq 40 && ${#sha_b} -eq 40 ]] || die "ab: --a and --b must be full 40-char SHAs (short ones stop resolving once the branch moves)"

select_stack
read_outputs
wait_for_ssh "$CLIENT_IP" "client"
if ssh_client "pgrep -f '[a]b-client\\.sh' >/dev/null"; then
    die "an A/B is still in progress on the client"
fi

RUN_ID="ab-$(date -u +%Y%m%d-%H%M%S)"
ssh_client 'cat > ab-client.sh' < "$SCRIPTS_DIR/ab-client.sh"
ssh_client "mkdir -p results/$RUN_ID; nohup bash -c \
    'bash ab-client.sh $SERVER_IP_PRIVATE $RUN_ID $sha_a $sha_b $pairs \"$concurrencies\"; echo \$? > results/$RUN_ID/exit_code' \
    > results/$RUN_ID/driver.log 2>&1 < /dev/null &"
mkdir -p "$RESULTS_ROOT"
echo "$RUN_ID" > "$RESULTS_ROOT/.current-run"
log "started $RUN_ID: a=$sha_a b=$sha_b, $pairs pairs at c in [$concurrencies]"
log "next: scripts/ab-status.sh"
