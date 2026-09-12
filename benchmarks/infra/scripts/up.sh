#!/usr/bin/env bash
# Provision the instances and prepare the server. Rerunnable; a no-op when everything exists.
#
# Usage: scripts/up.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

missing=()
for tool in pulumi aws node npm ssh scp curl; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
[[ ${#missing[@]} -eq 0 ]] || die "missing on PATH: ${missing[*]}"
[[ -f "$SSH_KEY" ]] || die "no SSH private key at $SSH_KEY (set SSH_KEY)"

select_stack
rm -f "$KNOWN_HOSTS"
log "pulumi up..."
pulumi up --yes
read_outputs
wait_for_ssh "$CLIENT_IP" "client"
wait_for_ssh "$SERVER_IP_PUBLIC" "server"
log "preparing the server (nginx + delay server)..."
ssh_server 'bash -s' < "$SCRIPTS_DIR/server-setup.sh"
log "up. client ubuntu@$CLIENT_IP, server ubuntu@$SERVER_IP_PUBLIC (private $SERVER_IP_PRIVATE)"
log "next: scripts/run.sh --ref <ref>"
