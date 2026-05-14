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

# Start test server
httpbin-start:
    docker run -d --name reqx-httpbin -p 80:80 kennethreitz/httpbin

# Stop test server
httpbin-stop:
    docker rm -f reqx-httpbin
