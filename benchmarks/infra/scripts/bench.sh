#!/usr/bin/env bash
# Orchestrates the full bench session:
#   1. pulumi up    (provision VPC, EC2 client + server, S3 results bucket)
#   2. server setup (clone repo + docker compose up)
#   3. client setup (rust + uv + build rqx + patch bench scripts)
#   4. run benches  (b1, b2, b8 × N runs each, capture to S3)
#   5. download     (mirror S3 results to ./results/<run-id>/ locally)
#   6. destroy      (pulumi destroy — prompts for confirmation)
#
# Flags:
#   --skip-up         skip pulumi up (assume infra already exists)
#   --skip-destroy    leave infra running after benches (you handle teardown)
#   --runs-per-bench N    override default (5)
#   --ref REF         git ref (branch, tag, or commit SHA) to bench. Default: main.
#
# Env (optional):
#   PULUMI_STACK      stack to use (default: dev)
#   AWS_PROFILE       AWS profile (default: whatever's active)
#   SSH_KEY           private key path (default: ~/.ssh/id_ed25519)
set -euo pipefail

# ---------- args & defaults ----------
SKIP_UP=false
SKIP_DESTROY=false
RUNS_PER_BENCH=5
REF="main"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-up) SKIP_UP=true; shift ;;
        --skip-destroy) SKIP_DESTROY=true; shift ;;
        --runs-per-bench) RUNS_PER_BENCH="$2"; shift 2 ;;
        --ref) REF="$2"; shift 2 ;;
        *) echo "unknown flag: $1" >&2; exit 1 ;;
    esac
done

PULUMI_STACK="${PULUMI_STACK:-dev}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
INFRA_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS_DIR="$INFRA_DIR/scripts"
RUN_ID="$(date -u +%Y%m%d-%H%M%S)"

SSH_OPTS=(
    -o StrictHostKeyChecking=accept-new
    -o UserKnownHostsFile=/dev/null
    -o ConnectTimeout=10
    -o ServerAliveInterval=30
    -i "$SSH_KEY"
)

log() { printf "\n[%s] %s\n" "$(date +%H:%M:%S)" "$*"; }

# ---------- pulumi up ----------
cd "$INFRA_DIR"
pulumi stack select "$PULUMI_STACK" >/dev/null

if ! $SKIP_UP; then
    log "pulumi up (this can take a few minutes)..."
    pulumi up --yes
fi

CLIENT_IP="$(pulumi stack output clientPublicIp)"
SERVER_IP_PRIVATE="$(pulumi stack output serverPrivateIp)"
SERVER_IP_PUBLIC="$(pulumi stack output serverPublicIp)"
BUCKET="$(pulumi stack output resultsBucketName)"

log "client public: $CLIENT_IP"
log "server private: $SERVER_IP_PRIVATE (public: $SERVER_IP_PUBLIC)"
log "results bucket: $BUCKET"
log "run id: $RUN_ID"
log "ref: $REF"

# ---------- wait for SSH ----------
wait_for_ssh() {
    local host="$1"
    local label="$2"
    log "waiting for SSH on $label ($host)..."
    for i in $(seq 1 60); do
        if ssh "${SSH_OPTS[@]}" -o BatchMode=yes "ubuntu@$host" "echo ready" >/dev/null 2>&1; then
            log "$label: SSH ready"
            return 0
        fi
        sleep 5
    done
    log "FATAL: $label never came up on SSH"
    exit 1
}

wait_for_ssh "$CLIENT_IP" "client"
wait_for_ssh "$SERVER_IP_PUBLIC" "server"

# ---------- server setup ----------
log "running server-setup.sh on $SERVER_IP_PUBLIC..."
ssh "${SSH_OPTS[@]}" "ubuntu@$SERVER_IP_PUBLIC" 'bash -s' < "$SCRIPTS_DIR/server-setup.sh"

# ---------- client setup ----------
log "running client-setup.sh on $CLIENT_IP (this is the long pole — ~10-15 min)..."
ssh "${SSH_OPTS[@]}" "ubuntu@$CLIENT_IP" "bash -s $SERVER_IP_PRIVATE $REF" < "$SCRIPTS_DIR/client-setup.sh"

# ---------- run benches ----------
log "running benches on $CLIENT_IP (run id: $RUN_ID)..."
ssh "${SSH_OPTS[@]}" "ubuntu@$CLIENT_IP" \
    "RUNS_PER_BENCH=$RUNS_PER_BENCH bash -s $RUN_ID $BUCKET" \
    < "$SCRIPTS_DIR/run-benches.sh"

# ---------- download ----------
LOCAL_RESULTS="$INFRA_DIR/results/$RUN_ID"
mkdir -p "$LOCAL_RESULTS"
log "downloading results from s3://$BUCKET/$RUN_ID/ to $LOCAL_RESULTS"
aws s3 sync "s3://$BUCKET/$RUN_ID/" "$LOCAL_RESULTS/"

log "results saved locally at $LOCAL_RESULTS"
ls -la "$LOCAL_RESULTS"

# ---------- destroy ----------
if $SKIP_DESTROY; then
    log "leaving infra up (per --skip-destroy)"
    log "  client: ssh ubuntu@$CLIENT_IP"
    log "  server: ssh ubuntu@$SERVER_IP_PUBLIC"
    log "  to teardown later: cd $INFRA_DIR && pulumi destroy"
    exit 0
fi

echo
read -r -p "Destroy infrastructure now? [y/N] " confirm
if [[ "$confirm" == "y" || "$confirm" == "Y" ]]; then
    log "running pulumi destroy..."
    pulumi destroy --yes
    log "all done. results at $LOCAL_RESULTS and s3://$BUCKET/$RUN_ID/"
else
    log "infra left running. teardown later with: cd $INFRA_DIR && pulumi destroy"
fi
