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

# Start test server
httpbin-start:
    docker run -d --name reqx-httpbin -p 80:80 kennethreitz/httpbin

# Stop test server
httpbin-stop:
    docker rm -f reqx-httpbin
