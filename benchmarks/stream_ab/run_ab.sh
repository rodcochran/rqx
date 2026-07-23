#!/usr/bin/env bash
#
# A/B the streaming path across two commits of rqx.
#
# Clones the PUBLIC repo and builds each ref from source, rather than diffing
# against a PyPI wheel. Two reasons: a released wheel contains every change
# since the release (not just this PR), and it was built by CI with its own
# profile — comparing against it would measure build configuration as much as
# code. Here both arms are built by the same toolchain, same flags, same
# container. The only variable is the source commit.
#
# Defaults are PR #139: base is the parent of the first PR commit.
set -euo pipefail

REPO="${REPO:-https://github.com/rodcochran/rqx.git}"
BASE_REF="${BASE_REF:-5e3fe3e812ba595265d01e089af2ae96aa5e69d1}"
HEAD_REF="${HEAD_REF:-6c83626a8afb882832121bcd6288782bcd6190e7}"
ROUNDS="${ROUNDS:-5}"
OUT_DIR="${OUT_DIR:-/results}"
RESULTS="${OUT_DIR}/raw.jsonl"

# mode | label | path | iterations | concurrency
#
# The first four rows are the optimization's target: large streams, where many
# chunks per response means many saved allocs. The last row is the ticket's
# no-regression check — small bodies at high concurrency, where the per-chunk
# saving is swamped by async machinery and we only care that nothing got worse.
CONFIGS=(
  "async 1mb  /1mb   120 1"
  "async 1mb  /1mb   240 8"
  "async 10mb /10mb  24  1"
  "sync  1mb  /1mb   120 1"
  "sync  1mb  /1mb   240 8"
  "sync  10mb /10mb  24  1"
  "async 8kb  /8kb   8000 64"
  "sync  8kb  /8kb   8000 64"
)

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }

build_arm() {
  local name=$1 ref=$2
  log "building '${name}' @ ${ref}"
  git clone --quiet "$REPO" "/src/${name}"
  git -C "/src/${name}" checkout --quiet "$ref"
  python -m venv "/venvs/${name}"
  "/venvs/${name}/bin/pip" install --quiet --upgrade pip
  (
    cd "/src/${name}"
    maturin build --release \
      --interpreter "/venvs/${name}/bin/python" \
      --out "/wheels/${name}" >/dev/null
  )
  "/venvs/${name}/bin/pip" install --quiet "/wheels/${name}"/*.whl
  log "'${name}' installed: $("/venvs/${name}/bin/python" -c 'import rqx; print(rqx.__file__)')"
}

main() {
  mkdir -p "$OUT_DIR" /src /venvs /wheels
  : >"$RESULTS"

  log "toolchain: $(rustc --version) | $(python --version)"
  nginx
  sleep 1
  curl -sf -o /dev/null http://127.0.0.1:8080/8kb || { echo "nginx not serving"; exit 1; }
  log "nginx up on 127.0.0.1:8080"

  build_arm base "$BASE_REF"
  build_arm head "$HEAD_REF"

  # Interleave arms WITHIN each round rather than running all of base then all
  # of head. Any drift over the session — thermal, VM scheduling, page cache —
  # then lands on both arms equally instead of biasing whichever ran second.
  for round in $(seq 1 "$ROUNDS"); do
    for cfg in "${CONFIGS[@]}"; do
      read -r mode label path iters conc <<<"$cfg"
      for arm in base head; do
        log "round ${round}/${ROUNDS} | ${arm} | ${mode} ${label} c=${conc}"
        "/venvs/${arm}/bin/python" /harness/bench_stream.py \
          --arm "$arm" --mode "$mode" --url "http://127.0.0.1:8080${path}" \
          --label "$label" --iterations "$iters" --concurrency "$conc" \
          --round "$round" >>"$RESULTS"
      done
    done
  done

  log "raw records: ${RESULTS}"
  python /harness/compare.py "$RESULTS" \
    --json "${OUT_DIR}/summary.json" \
    --base-ref "$BASE_REF" --head-ref "$HEAD_REF" --rounds "$ROUNDS" \
    --toolchain "$(rustc --version)" \
    | tee "${OUT_DIR}/summary.txt"
}

main "$@"
