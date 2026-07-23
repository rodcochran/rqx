default:
    @just --list

# First-time setup: deps + initial build
setup: install-python-deps build

# Install Python deps via uv and generate lockfile
install-python-deps:
    uv venv
    uv pip install -e ".[dev]"
    uv lock

# Build the extension (debug; fast for iteration)
build:
    maturin develop

# Build the extension in release mode (for benchmarks and TLS tests)
build-release:
    maturin develop --release

# Run the test suite in parallel
test: build
    uv run pytest tests/ -n 8

# Regenerate test certificates from scratch
regen-certs:
    rm -rf tests/ssl/certs tests/ssl/.cert-gen.lock
    bash tests/ssl/generate_certs.sh

# Lint Rust + Python
lint:
    cargo clippy
    ruff check python/

# Type check Python
typecheck:
    uv run ty check python/

# Full pre-push verification
check: lint typecheck test

# A/B the streaming path across two commits (issue #108 / PR #139).
# Builds BOTH refs from source in a Linux container so the toolchain is not a
# variable; writes results/summary.json + summary.txt + raw.jsonl.
# First run is slow (~10-20 min): rustup plus two --release builds.
bench-stream rounds="5" base_ref="5e3fe3e812ba595265d01e089af2ae96aa5e69d1" head_ref="6c83626a8afb882832121bcd6288782bcd6190e7":
    docker build -t rqx-stream-ab benchmarks/stream_ab
    mkdir -p benchmarks/stream_ab/results
    docker run --rm \
        -e ROUNDS={{rounds}} -e BASE_REF={{base_ref}} -e HEAD_REF={{head_ref}} \
        -v "{{justfile_directory()}}/benchmarks/stream_ab/results:/results" \
        rqx-stream-ab
    @echo "\nfull working -> benchmarks/stream_ab/results/summary-sweep.txt"

# Fast smoke of the streaming A/B (1 round) — checks the harness runs at all
# before committing to the full sweep. Not enough rounds to trust the verdicts.
bench-stream-smoke: (bench-stream "1")

# Drill into ONE cell with many rounds. Power comes from rounds, and rounds are
# cheapest spent on a single config. Cell is "<mode> <payload> <concurrency>",
# e.g. `just bench-stream-cell "async 1mb 8" 40`. Keep rounds EVEN so the
# alternating arm order stays balanced.
bench-stream-cell cell rounds="40" base_ref="5e3fe3e812ba595265d01e089af2ae96aa5e69d1" head_ref="6c83626a8afb882832121bcd6288782bcd6190e7":
    docker build -t rqx-stream-ab benchmarks/stream_ab
    mkdir -p benchmarks/stream_ab/results
    docker run --rm \
        -e ROUNDS={{rounds}} -e FILTER="{{cell}}" \
        -e BASE_REF={{base_ref}} -e HEAD_REF={{head_ref}} \
        -v "{{justfile_directory()}}/benchmarks/stream_ab/results:/results" \
        rqx-stream-ab
    @echo "\nfull working -> benchmarks/stream_ab/results/summary-cell-*.txt"

# Start test server
httpbin-start:
    docker run -d --name reqx-httpbin -p 80:80 kennethreitz/httpbin

# Stop test server
httpbin-stop:
    docker rm -f reqx-httpbin
