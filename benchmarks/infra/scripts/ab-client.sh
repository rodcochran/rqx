#!/usr/bin/env bash
# Runs on the bench client. Builds two refs into their own venvs, then alternates
# b1 samples between them so box drift and position can't masquerade as a code delta.
#
# Usage: ab-client.sh <SERVER_PRIVATE_IP> <RUN_ID> <SHA_A> <SHA_B> <PAIRS> "<CONCURRENCIES>"
# Output: ~/results/<RUN_ID>/ab.log with one line per sample:
#   AB  build=<a|b> c=<n> pair=<n> pos=<1|2> <b1 json>
#   CTL client=<httpr|aiohttp> c=<n> at=<start|mid|end> <b1 json>
set -euo pipefail

SERVER_IP="$1"; RUN_ID="$2"; SHA_A="$3"; SHA_B="$4"; PAIRS="$5"; CONCURRENCIES="$6"
WARMUP=2
MEASURE=8
AB="$HOME/ab"
RESULTS="$HOME/results/$RUN_ID"
LOG="$RESULTS/ab.log"
mkdir -p "$AB" "$RESULTS"

say() { echo "[ab-client $(date -u +%H:%M:%S)] $*"; }

sudo cloud-init status --wait >/dev/null || say "cloud-init reported failure; continuing"
if ! command -v cargo >/dev/null; then
    say "installing rustup + cargo..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
if ! command -v uv >/dev/null; then
    say "installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
fi
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

cd "$HOME"
[ -d rqx ] || git clone https://github.com/rodcochran/rqx.git
cd rqx
git fetch origin "$SHA_A" "$SHA_B"

# One venv with maturin to build both wheels; one venv per build to run them.
[ -d "$AB/tools" ] || uv venv "$AB/tools"
uv pip install --python "$AB/tools/bin/python" maturin

build() {
    local name="$1" sha="$2"
    if [ -d "$AB/venv-$name" ]; then
        say "$name already built ($(cat "$AB/sha-$name"))"
        return
    fi
    say "building $name ($sha)..."
    git checkout --quiet "$sha"
    "$AB/tools/bin/maturin" build --release -o "$AB/wheels-$name" >"$RESULTS/build-$name.log" 2>&1
    uv venv "$AB/venv-$name"
    uv pip install --python "$AB/venv-$name/bin/python" "$AB"/wheels-"$name"/*.whl
    echo "$sha" > "$AB/sha-$name"
}
build a "$SHA_A"
build b "$SHA_B"

# The controls get their own venv; the bench scripts come from ref B's checkout.
[ -d "$AB/venv-ctl" ] || uv venv "$AB/venv-ctl"
uv pip install --python "$AB/venv-ctl/bin/python" httpr aiohttp
git checkout --quiet "$SHA_B"
cd benchmarks
for f in b1_rqx.py b1_httpr.py b1_aiohttp.py; do
    sed -i "s|http://localhost:8080|http://${SERVER_IP}:8080|g" "$f"
done
curl -sf -o /dev/null "http://${SERVER_IP}:8080/json" || { say "FATAL: can't reach the server"; exit 1; }

{
    echo "run_id=$RUN_ID"
    echo "sha_a=$SHA_A"
    echo "sha_b=$SHA_B"
    echo "pairs=$PAIRS"
    echo "concurrencies=$CONCURRENCIES"
    echo "warmup=$WARMUP measure=$MEASURE"
    echo "rustc=$(rustc --version)"
    echo "python=$("$AB/venv-a/bin/python" --version)"
    echo "venv_a=$("$AB/venv-a/bin/python" -m pip list 2>/dev/null | grep -i rqx || "$AB/venv-a/bin/python" -c 'import rqx; print("rqx", rqx.__version__)')"
    echo "venv_b=$("$AB/venv-b/bin/python" -c 'import rqx; print("rqx", rqx.__version__)')"
    echo "controls=$("$AB/venv-ctl/bin/python" -c 'import httpr, aiohttp; print("httpr", httpr.__version__, "aiohttp", aiohttp.__version__)')"
    echo "cargo_lock_diff_lines=$(cd .. && git diff --stat "$SHA_A" "$SHA_B" -- Cargo.lock | tail -1)"
} > "$RESULTS/metadata.txt"

sample() {
    local build="$1" c="$2" pair="$3" pos="$4"
    local json
    json="$("$AB/venv-$build/bin/python" -u b1_rqx.py --c "$c" --warmup $WARMUP --measure $MEASURE --run "$pair" 2>/dev/null)" \
        || json='{"skipped":"crashed"}'
    echo "AB  build=$build c=$c pair=$pair pos=$pos $json" | tee -a "$LOG"
    sleep 3
}

control() {
    local at="$1" c="$2"
    for client in httpr aiohttp; do
        local json
        json="$("$AB/venv-ctl/bin/python" -u "b1_$client.py" --c "$c" --warmup $WARMUP --measure $MEASURE 2>/dev/null)" \
            || json='{"skipped":"crashed"}'
        echo "CTL client=$client c=$c at=$at $json" | tee -a "$LOG"
        sleep 3
    done
}

say "benching: $PAIRS pairs at c in [$CONCURRENCIES], ${WARMUP}s warmup + ${MEASURE}s measure per sample"
for c in $CONCURRENCIES; do
    control start "$c"
    for pair in $(seq 1 "$PAIRS"); do
        # Counterbalanced: b first on odd pairs, a first on even ones.
        if (( pair % 2 == 1 )); then
            sample b "$c" "$pair" 1; sample a "$c" "$pair" 2
        else
            sample a "$c" "$pair" 1; sample b "$c" "$pair" 2
        fi
        if (( pair == PAIRS / 2 )); then control mid "$c"; fi
    done
    control end "$c"
done
say "done"
