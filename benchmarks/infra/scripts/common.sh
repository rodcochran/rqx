#!/usr/bin/env bash
# Shared by the session scripts (up, run, status, wait, collect, destroy). Source, don't run.
#
# Env (optional): PULUMI_STACK (dev), SSH_KEY (~/.ssh/id_ed25519), AWS_PROFILE (the stack's
# aws:profile), PULUMI_BACKEND_URL (file://~), PULUMI_CONFIG_PASSPHRASE (blank).
set -euo pipefail

INFRA_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR="$(cd "$INFRA_DIR/../.." && pwd)"
SCRIPTS_DIR="$INFRA_DIR/scripts"
RESULTS_ROOT="$INFRA_DIR/results"
export PULUMI_BACKEND_URL="${PULUMI_BACKEND_URL:-file://~}"
export PULUMI_CONFIG_PASSPHRASE="${PULUMI_CONFIG_PASSPHRASE-}"
PULUMI_STACK="${PULUMI_STACK:-dev}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
# Host keys are accepted on first contact after `up` and verified on every call after.
KNOWN_HOSTS="$INFRA_DIR/.known_hosts.$PULUMI_STACK"
SSH_OPTS=(
    -o StrictHostKeyChecking=accept-new
    -o "UserKnownHostsFile=$KNOWN_HOSTS"
    -o ConnectTimeout=10
    -o ServerAliveInterval=30
    -o LogLevel=ERROR
    -i "$SSH_KEY"
)

log() { printf "[%s] %s\n" "$(date +%H:%M:%S)" "$*"; }
die() { echo "bench: $*" >&2; exit 1; }

select_stack() {
    cd "$INFRA_DIR"
    pulumi stack select "$PULUMI_STACK" >/dev/null 2>&1 \
        || die "stack '$PULUMI_STACK' not found; run scripts/setup.sh --profile <aws-profile> first"
    export AWS_PROFILE="${AWS_PROFILE:-$(pulumi config get aws:profile 2>/dev/null || true)}"
}

read_outputs() {
    CLIENT_IP="$(pulumi stack output clientPublicIp 2>/dev/null)" || die "no instances; run scripts/up.sh first"
    SERVER_IP_PRIVATE="$(pulumi stack output serverPrivateIp)"
    SERVER_IP_PUBLIC="$(pulumi stack output serverPublicIp)"
    BUCKET="$(pulumi stack output resultsBucketName)"
}

# Instance ids in the stack, and "instance-id public-ip" per elastic IP.
instance_ids() { pulumi stack export | jq -r '.deployment.resources[]? | select(.type=="aws:ec2/instance:Instance") | .id'; }
eip_instances() { pulumi stack export | jq -r '.deployment.resources[]? | select(.type=="aws:ec2/eip:Eip") | "\(.outputs.instance) \(.outputs.publicIp)"'; }

ssh_client() { ssh "${SSH_OPTS[@]}" "ubuntu@$CLIENT_IP" "$@"; }
ssh_server() { ssh "${SSH_OPTS[@]}" "ubuntu@$SERVER_IP_PUBLIC" "$@"; }

wait_for_ssh() {
    local host="$1" label="$2"
    log "waiting for SSH on $label ($host)..."
    for _ in $(seq 1 60); do
        if ssh "${SSH_OPTS[@]}" -o BatchMode=yes "ubuntu@$host" true 2>/dev/null; then
            return 0
        fi
        sleep 5
    done
    die "$label never came up on SSH"
}

# The run id to act on: the argument if given, else the last one started from this checkout.
resolve_run() {
    RUN_ID="${1:-}"
    if [[ -z "$RUN_ID" ]]; then
        [[ -s "$RESULTS_ROOT/.current-run" ]] || die "no run id given and none recorded; pass one"
        RUN_ID="$(cat "$RESULTS_ROOT/.current-run")"
    fi
    LOCAL_RESULTS="$RESULTS_ROOT/$RUN_ID"
}

sync_down() {
    mkdir -p "$LOCAL_RESULTS"
    scp "${SSH_OPTS[@]}" -q -r "ubuntu@$CLIENT_IP:results/$RUN_ID/." "$LOCAL_RESULTS/" \
        || die "could not copy results/$RUN_ID from the client"
}

run_finished() { [[ -f "$LOCAL_RESULTS/exit_code" ]]; }

# The bracket keeps the pattern from matching the shell that runs the pgrep.
run_in_progress() { ssh_client "pgrep -f '[r]un-benches\\.sh' >/dev/null"; }

# Same python the repo uses, so plot_bench's matplotlib is there.
repo_python() {
    if [[ -x "$REPO_DIR/.venv/bin/python" ]]; then echo "$REPO_DIR/.venv/bin/python"; else echo python3; fi
}
