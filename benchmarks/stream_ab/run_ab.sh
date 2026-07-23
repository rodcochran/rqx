#!/usr/bin/env bash
#
# Benchmarks two commits of rqx head to head.
#
# Both are built from source in this container with one toolchain, so the only
# variable is the source commit. Defaults are PR #139, with base as the parent
# of its first commit.
set -euo pipefail

REPO="${REPO:-https://github.com/rodcochran/rqx.git}"
BASE_REF="${BASE_REF:-5e3fe3e812ba595265d01e089af2ae96aa5e69d1}"
HEAD_REF="${HEAD_REF:-6c83626a8afb882832121bcd6288782bcd6190e7}"
ROUNDS="${ROUNDS:-5}"
OUT_DIR="${OUT_DIR:-/results}"

# Restrict the sweep to one config, e.g. FILTER="async 1mb 8". Lets one config
# get many rounds without paying for the full matrix.
FILTER="${FILTER:-}"

# Filtered runs write to their own files so a drill-down never clobbers the
# sweep you are comparing it against.
if [[ -n "$FILTER" ]]; then
  SLUG="config-$(printf '%s' "$FILTER" | tr ' /' '--')"
else
  SLUG="sweep"
fi
RESULTS="${OUT_DIR}/raw-${SLUG}.jsonl"
SUMMARY_JSON="${OUT_DIR}/summary-${SLUG}.json"
SUMMARY_TXT="${OUT_DIR}/summary-${SLUG}.txt"

# mode | label | path | iterations | concurrency
#
# Large streams are where the saving should show. The 8kb rows are the
# no-regression check: too small for the effect, so they should not move.
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

install_build() {
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

  # Fail loudly on a FILTER typo rather than silently producing an empty run.
  if [[ -n "$FILTER" ]]; then
    matched=0
    for cfg in "${CONFIGS[@]}"; do
      read -r mode label _path _iters conc <<<"$cfg"
      [[ "$mode $label $conc" == "$FILTER" ]] && matched=1
    done
    if (( ! matched )); then
      echo "FILTER '${FILTER}' matched no config. Available:" >&2
      for cfg in "${CONFIGS[@]}"; do
        read -r mode label _path _iters conc <<<"$cfg"
        echo "  ${mode} ${label} ${conc}" >&2
      done
      exit 1
    fi
    log "FILTER active — only '${FILTER}'"
  fi

  log "toolchain: $(rustc --version) | $(python --version)"
  nginx
  sleep 1
  curl -sf -o /dev/null http://127.0.0.1:8080/8kb || { echo "nginx not serving"; exit 1; }
  log "nginx up on 127.0.0.1:8080"

  install_build base "$BASE_REF"
  install_build head "$HEAD_REF"

  # Interleave builds within each round so session drift lands on both equally
  # instead of biasing whichever ran second.
  for round in $(seq 1 "$ROUNDS"); do
    for cfg in "${CONFIGS[@]}"; do
      read -r mode label path iters conc <<<"$cfg"
      if [[ -n "$FILTER" && "$mode $label $conc" != "$FILTER" ]]; then
        continue
      fi
      # Alternate which build goes first. Always running base first would let
      # whatever advantages the second slot (warm caches, ramped CPU) land in
      # the delta as a fake result. Keep ROUNDS even so the two orders balance.
      if (( round % 2 == 0 )); then order="head base"; else order="base head"; fi
      for build in $order; do
        log "round ${round}/${ROUNDS} | ${build} | ${mode} ${label} c=${conc}"
        "/venvs/${build}/bin/python" /harness/bench_stream.py \
          --build "$build" --mode "$mode" --url "http://127.0.0.1:8080${path}" \
          --label "$label" --iterations "$iters" --concurrency "$conc" \
          --round "$round" >>"$RESULTS"
      done
    done
  done

  log "raw records: ${RESULTS}"
  # Full working to the saved file, plain verdict to the terminal.
  python /harness/compare.py "$RESULTS" --detail \
    --json "$SUMMARY_JSON" \
    --base-ref "$BASE_REF" --head-ref "$HEAD_REF" --rounds "$ROUNDS" \
    --toolchain "$(rustc --version)" >"$SUMMARY_TXT"
  python /harness/compare.py "$RESULTS"
}

main "$@"
