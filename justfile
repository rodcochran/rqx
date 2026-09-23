default:
    @just --list

# Bench recipes live in benchmarks/justfile: `just benchmarks::up`, `just benchmarks::status`, ...
mod benchmarks

# First-time setup: deps + initial build
setup: install-python-deps build

# Install Python deps via uv and generate lockfile
install-python-deps:
    uv venv
    uv sync
    uv lock

# Build the extension (debug; fast for iteration)
build:
    maturin develop

# Build the extension the way a release wheel is built (for benchmarks and TLS tests)
build-release:
    maturin develop --profile release-lto

# Run the test suite in parallel
test: build
    uv run pytest tests/ -n 8

# Fixture-server tests only (no docker needed)
test-unit:
    uv run pytest tests/unit -n 8

# Starts httpbin in Docker via testcontainers (or set RQX_HTTPBIN_URL)
test-integration:
    uv run pytest tests/integration -n 8

# Hypothesis tests; HYPOTHESIS_PROFILE=nightly for the bigger budget
test-property:
    uv run pytest tests/property -n 8

# Same tests against httpx and rqx; known divergences are strict xfails
test-equivalence:
    uv run pytest tests/equivalence -n 8

# Regenerate test certificates from scratch
regen-certs:
    rm -rf tests/ssl/certs tests/ssl/.cert-gen.lock
    bash tests/ssl/generate_certs.sh

# Lint Rust + Python
lint:
    cargo clippy
    uv run ruff check python/ tests/

# Type check Python
typecheck:
    uv run ty check python/

# Full pre-push verification
check: lint typecheck test

# Compare two commits on the streaming path (issue #108 / PR #139).
# Runs every config by default, or one of them:
#     just bench-stream
#     just bench-stream 20
#     just bench-stream 40 "async 1mb 8"
# Keep rounds even so the alternating build order stays balanced.
# The first run takes 10-20 minutes: it installs Rust and does two release builds.
bench-stream rounds="10" only="" base_ref="5e3fe3e812ba595265d01e089af2ae96aa5e69d1" head_ref="6c83626a8afb882832121bcd6288782bcd6190e7":
    docker build -t rqx-stream-ab benchmarks/stream_ab
    mkdir -p benchmarks/stream_ab/results
    docker run --rm \
        -e ROUNDS={{rounds}} -e ONLY="{{only}}" \
        -e BASE_REF={{base_ref}} -e HEAD_REF={{head_ref}} \
        -v "{{justfile_directory()}}/benchmarks/stream_ab/results:/results" \
        rqx-stream-ab

