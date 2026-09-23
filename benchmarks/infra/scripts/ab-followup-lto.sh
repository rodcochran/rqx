#!/usr/bin/env bash
# Runs on the bench client after ab-followup.sh. Builds arm `d`: ref B with fat LTO and one
# codegen unit, then pairs d vs a at the first concurrency and a vs c at the second.
#
# Usage: ab-followup-lto.sh <RUN_ID> <SHA_B> <PAIRS> <C_LTO> <C_RSS>
set -euo pipefail

RUN_ID="$1"; SHA_B="$2"; PAIRS="$3"; C_LTO="$4"; C_RSS="$5"
WARMUP=2
MEASURE=8
AB="$HOME/ab"
RESULTS="$HOME/results/$RUN_ID"
LOG="$RESULTS/ab.log"
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

say() { echo "[ab-followup-lto $(date -u +%H:%M:%S)] $*"; }

cd "$HOME/rqx"
if [ ! -d "$AB/venv-d" ]; then
    say "building d: $SHA_B with lto=fat, codegen-units=1..."
    git checkout --quiet "$SHA_B"
    CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
        "$AB/tools/bin/maturin" build --release -o "$AB/wheels-d" >"$RESULTS/build-d.log" 2>&1
    uv venv "$AB/venv-d"
    uv pip install --python "$AB/venv-d/bin/python" "$AB"/wheels-d/*.whl
    ls -l "$AB"/wheels-*/*.whl | awk '{print "wheel:", $5, $9}' >> "$RESULTS/metadata.txt"
fi
cd benchmarks

sample() {
    local build="$1" c="$2" pair="$3" pos="$4" vs="$5"
    local json
    json="$("$AB/venv-$build/bin/python" -u b1_rqx.py --c "$c" --warmup $WARMUP --measure $MEASURE --run "$pair" 2>/dev/null)" \
        || json='{"skipped":"crashed"}'
    echo "AB  build=$build c=$c pair=$pair pos=$pos vs=$vs $json" | tee -a "$LOG"
    sleep 3
}

run_pairs() {
    local candidate="$1" baseline="$2" c="$3"
    say "$PAIRS pairs of $candidate vs $baseline at c=$c"
    for pair in $(seq 1 "$PAIRS"); do
        if (( pair % 2 == 1 )); then
            sample "$candidate" "$c" "$pair" 1 "$candidate$baseline"; sample "$baseline" "$c" "$pair" 2 "$candidate$baseline"
        else
            sample "$baseline" "$c" "$pair" 1 "$candidate$baseline"; sample "$candidate" "$c" "$pair" 2 "$candidate$baseline"
        fi
    done
}

run_pairs d a "$C_LTO"
run_pairs a c "$C_RSS"
say "done"
