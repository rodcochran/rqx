#!/usr/bin/env bash
# Runs on the bench client. Installs Rust + uv, clones rqx, builds the
# extension in release mode, installs the benchmark deps (rqx + httpx +
# aiohttp), and patches the target benches to point at the remote server.
#
# Usage (via the orchestrator): client-setup.sh <SERVER_PRIVATE_IP> [REF]
#   REF: git ref to bench (branch name, tag, or commit SHA). Defaults to main.
set -euo pipefail

SERVER_IP="${1:?usage: client-setup.sh <SERVER_PRIVATE_IP> [REF]}"
REF="${2:-main}"

echo "[client-setup] waiting for cloud-init to finish..."
# `--wait` exits non-zero if cloud-init itself failed (e.g., an apt install
# in user_data couldn't resolve a package name). We don't actually care here —
# if the user has manually recovered the box, we want to keep going. The
# subsequent steps (`apt list --installed`-style checks via `command -v cargo`
# and `which python3`) will fail loudly if something's actually missing.
sudo cloud-init status --wait >/dev/null || echo "[client-setup] cloud-init reported failure; continuing anyway"

# Rust toolchain
if ! command -v cargo >/dev/null; then
    echo "[client-setup] installing rustup + cargo..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"

# uv (fast Python package manager)
if ! command -v uv >/dev/null; then
    echo "[client-setup] installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
fi
# shellcheck disable=SC1091
. "$HOME/.local/bin/env" 2>/dev/null || true
export PATH="$HOME/.local/bin:$PATH"

cd "$HOME"
# `--depth 1 --branch $REF` works for branch names and tags but not arbitrary
# commit SHAs. If REF is a SHA, fall through to fetch-after-clone.
if [ ! -d rqx ]; then
    if ! git clone --depth 1 --branch "$REF" https://github.com/rodcochran/rqx.git 2>/dev/null; then
        echo "[client-setup] shallow clone of $REF failed (probably a SHA); doing full clone + checkout"
        git clone https://github.com/rodcochran/rqx.git
        (cd rqx && git checkout "$REF")
    fi
else
    # Pull latest so subsequent re-runs pick up bench-script changes you've
    # pushed since the last invocation.
    (cd rqx && git fetch origin "$REF" && git reset --hard FETCH_HEAD)
fi

cd "$HOME/rqx"
echo "[client-setup] benchmarking $(git rev-parse --short HEAD) ($(git log -1 --pretty=%s))"

# Create venv + install deps if not done
if [ ! -d .venv ]; then
    uv venv
fi
# shellcheck disable=SC1091
. .venv/bin/activate
uv pip install -e ".[dev,benchmarks]" httpx aiohttp httpr

# Build rqx in release mode (the long pole — ~5-10 min on cold cargo cache).
echo "[client-setup] building rqx in release mode (this takes a while)..."
maturin develop --release

# Patch the target benches to hit the server's private IP instead of
# localhost. nginx is on :8080 on the server (matches docker-compose.yaml).
echo "[client-setup] patching bench scripts to target $SERVER_IP:8080..."
cd "$HOME/rqx/benchmarks"
for f in \
    b1_rqx.py b1_httpr.py b1_httpx.py b1_aiohttp.py \
    b2_latency.py \
    b8_concurrency_sweep.py; do
    sed -i "s|http://localhost:8080|http://${SERVER_IP}:8080|g" "$f"
    if ! grep -q "http://${SERVER_IP}:8080" "$f"; then
        echo "[client-setup] FATAL: sed didn't patch $f"
        exit 1
    fi
done

# Sanity-check the network path
if ! curl -sf -o /dev/null "http://${SERVER_IP}:8080/json"; then
    echo "[client-setup] FATAL: can't reach server at http://${SERVER_IP}:8080"
    exit 1
fi

echo "[client-setup] done"
