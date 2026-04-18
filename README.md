# reqx

A Rust-backed Python HTTP client with an httpx-compatible API.

reqx replaces httpx's pure-Python internals with a Rust core built on reqwest and tokio, delivering significantly better throughput, latency, and memory usage under concurrent load — while keeping the API familiar.

## Highlights

- **Sync and async clients** with the same API as httpx
- **All HTTP methods**: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS
- **Retry system** with exponential backoff, Retry-After header support, and total timeout budgets — executing entirely in Rust
- **Connection pooling** with configurable limits
- **HTTP/2 support**
- **Streaming responses** via `iter_bytes()`
- **Cookie persistence** across requests
- **Custom exception hierarchy** matching httpx semantics

## Quick look

```python
import reqx

# Sync
with reqx.Client() as client:
    resp = client.get("https://httpbin.org/get")
    print(resp.json())

# Async
async with reqx.AsyncClient() as client:
    resp = await client.get("https://httpbin.org/get")
    print(resp.json())

# With retries
transport = reqx.HTTPTransport(
    retries=reqx.Retry(total=3, backoff_factor=0.5, status_forcelist={503}),
)
with reqx.Client(transport=transport) as client:
    resp = client.get("https://example.com/api")
```

## Performance

Benchmarked against httpx and aiohttp on a local nginx instance serving a 1KB JSON payload:

| Concurrency | reqx (RPS) | httpx (RPS) | aiohttp (RPS) |
| ----------- | ---------- | ----------- | ------------- |
| 10          | 5,368      | 947         | 6,149         |
| 100         | 6,105      | 126         | 7,821         |
| 1,000       | 6,387      | 3           | 7,317         |

See [docs/report.md](docs/report.md) for full benchmark results including latency, memory, and JSON parsing analysis.

## Installation

reqx is not yet published on PyPI. To build from source:

```bash
# Prerequisites: Rust toolchain, Python 3.9+, uv
pip install uv

# Build and install in development mode
git clone https://github.com/rodcochran/reqx.git
cd reqx
uv venv
source .venv/bin/activate
uv pip install maturin
maturin develop
uv pip install -e ".[dev]"
```

## Project goals

This project set out to answer a question: can you take httpx's excellent API and back it with Rust to eliminate the structural performance bottlenecks of pure-Python HTTP?

The answer is yes. Under high concurrency, reqx delivers ~2,000x the throughput of httpx while using ~93x less Python memory. It stays competitive with aiohttp (which uses C extensions) across all workloads.

reqx was built as a learning project for pyo3 and maturin. The code is functional but rough — contributions are welcome.

## Contributing

This project is early and there's plenty to improve:

- MockTransport for network-free testing
- Granular timeout configuration (connect, read, write)
- Streaming + redirect support
- Documentation and type stubs
- CI/CD and PyPI publishing
- Code cleanup and deduplication

If you're interested in Rust, Python FFI, or HTTP internals, this is a good codebase to learn from. Open an issue or submit a PR.

## Acknowledgements

reqx builds on the work of several excellent projects:

- [httpx](https://github.com/encode/httpx) — the API design this project aims to match
- [pyo3](https://github.com/PyO3/pyo3) — Rust/Python FFI framework
- [maturin](https://github.com/PyO3/maturin) — build system for Rust Python extensions
- [reqwest](https://github.com/seanmonstar/reqwest) — the Rust HTTP client powering reqx
- [tokio](https://github.com/tokio-rs/tokio) — async runtime

## License

MIT
