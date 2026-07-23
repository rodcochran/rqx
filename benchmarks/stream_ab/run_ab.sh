#!/usr/bin/env bash
#
# Builds two commits of rqx from source, then hands off to run.py.
#
# Both are built in this container with one toolchain, so the only variable is
# the source commit. Defaults are PR #139, with base as the parent of its first
# commit.
set -euo pipefail

REPO="${REPO:-https://github.com/rodcochran/rqx.git}"
BASE_REF="${BASE_REF:-5e3fe3e812ba595265d01e089af2ae96aa5e69d1}"
HEAD_REF="${HEAD_REF:-6c83626a8afb882832121bcd6288782bcd6190e7}"
ROUNDS="${ROUNDS:-10}"
ONLY="${ONLY:-}"
OUT_DIR="${OUT_DIR:-/results}"

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
}

main() {
  mkdir -p "$OUT_DIR" /src /venvs /wheels

  log "toolchain: $(rustc --version) | $(python --version)"
  nginx
  sleep 1
  curl -sf -o /dev/null http://127.0.0.1:8080/8kb || { echo "nginx not serving"; exit 1; }

  install_build base "$BASE_REF"
  install_build head "$HEAD_REF"

  python /harness/run.py --rounds "$ROUNDS" --only "$ONLY" --out "$OUT_DIR"
}

main "$@"
