#!/usr/bin/env just --justfile

# Build the extension
build:
    maturin develop

# Run tests
# Start test server
httpbin:
    docker run -d --name reqx-httpbin -p 80:80 kennethreitz/httpbin

# Stop test server
httpbin-stop:
    docker rm -f reqx-httpbin
test: build
    pytest tests/* -n 8

# Run benchmarks
bench: build
    python benchmarks/b1_throughput.py

# Lint
lint:
    cargo clippy
    ruff check python/

# Type check
typecheck:
    ty check python/