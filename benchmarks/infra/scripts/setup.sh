#!/usr/bin/env bash
# One-time setup for the bench stack: installs the Pulumi program's deps and
# writes the per-operator stack config (AWS profile, your IP, your SSH key).
#
# Usage: scripts/setup.sh --profile <aws-profile> [--stack dev] [--ssh-key ~/.ssh/id_ed25519]
#
# Uses Pulumi's local file backend with a blank passphrase unless you export
# PULUMI_BACKEND_URL / PULUMI_CONFIG_PASSPHRASE yourself, so no Pulumi account
# is needed and your global `pulumi login` is left alone.
set -euo pipefail

PROFILE=""
STACK="${PULUMI_STACK:-dev}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile) PROFILE="$2"; shift 2 ;;
        --stack) STACK="$2"; shift 2 ;;
        --ssh-key) SSH_KEY="$2"; shift 2 ;;
        *) echo "unknown flag: $1" >&2; exit 1 ;;
    esac
done
[[ -n "$PROFILE" ]] || { echo "usage: setup.sh --profile <aws-profile> [--stack dev] [--ssh-key path]" >&2; exit 1; }
[[ -f "$SSH_KEY.pub" ]] || { echo "no public key at $SSH_KEY.pub (pass --ssh-key)" >&2; exit 1; }

export PULUMI_BACKEND_URL="${PULUMI_BACKEND_URL:-file://~}"
export PULUMI_CONFIG_PASSPHRASE="${PULUMI_CONFIG_PASSPHRASE-}"

INFRA_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$INFRA_DIR"

echo "[setup] installing Pulumi program deps..."
npm install --silent

if ! pulumi stack select "$STACK" 2>/dev/null; then
    echo "[setup] creating stack $STACK..."
    pulumi stack init "$STACK"
fi

MY_IP="$(curl -sf https://api.ipify.org)"
echo "[setup] profile=$PROFILE  ssh from $MY_IP/32  key=$SSH_KEY.pub"
pulumi config set aws:profile "$PROFILE"
pulumi config set sshAllowedCidr "$MY_IP/32"
pulumi config set sshPublicKey "$(cat "$SSH_KEY.pub")"

echo "[setup] done. Pulumi.$STACK.yaml written (gitignored). Next: scripts/bench.sh"
