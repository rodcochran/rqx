#!/usr/bin/env bash
# Runs on the bench client after ab-client.sh. Builds a third arm, `c`: ref B's source with
# ref A's Cargo.lock, so the split and the dependency bumps can be told apart. Then alternates
# pairs of a vs c (split only) and b vs c (deps only) at the given concurrency.
#
# Usage: ab-followup.sh <RUN_ID> <SHA_A> <SHA_B> <PAIRS> <CONCURRENCY>
# Output: appends to ~/results/<RUN_ID>/ab.log with build=c samples, using a fresh pair numbering
#   AB  build=<a|b|c> c=<n> pair=<n> pos=<1|2> vs=<ac|bc> <b1 json>
set -euo pipefail

RUN_ID="$1"; SHA_A="$2"; SHA_B="$3"; PAIRS="$4"; C="$5"
WARMUP=2
MEASURE=8
AB="$HOME/ab"
RESULTS="$HOME/results/$RUN_ID"
LOG="$RESULTS/ab.log"
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

say() { echo "[ab-followup $(date -u +%H:%M:%S)] $*"; }

cd "$HOME/rqx"
if [ ! -d "$AB/venv-c" ]; then
    say "building c: $SHA_B source with $SHA_A Cargo.lock..."
    git checkout --quiet "$SHA_B"
    git show "$SHA_A:Cargo.lock" > Cargo.lock
    "$AB/tools/bin/maturin" build --release -o "$AB/wheels-c" >"$RESULTS/build-c.log" 2>&1
    grep -A1 '^name = "hyper"$' Cargo.lock | tail -1 | sed 's/^/hyper in arm c: /' >> "$RESULTS/metadata.txt"
    git checkout --quiet -- Cargo.lock
    uv venv "$AB/venv-c"
    uv pip install --python "$AB/venv-c/bin/python" "$AB"/wheels-c/*.whl
fi
cd benchmarks

sample() {
    local build="$1" pair="$2" pos="$3" vs="$4"
    local json
    json="$("$AB/venv-$build/bin/python" -u b1_rqx.py --c "$C" --warmup $WARMUP --measure $MEASURE --run "$pair" 2>/dev/null)" \
        || json='{"skipped":"crashed"}'
    echo "AB  build=$build c=$C pair=$pair pos=$pos vs=$vs $json" | tee -a "$LOG"
    sleep 3
}

for vs in ac bc; do
    other="${vs:0:1}"
    say "$PAIRS pairs of $other vs c at c=$C"
    for pair in $(seq 1 "$PAIRS"); do
        if (( pair % 2 == 1 )); then
            sample c "$pair" 1 "$vs"; sample "$other" "$pair" 2 "$vs"
        else
            sample "$other" "$pair" 1 "$vs"; sample c "$pair" 2 "$vs"
        fi
    done
done
say "done"
