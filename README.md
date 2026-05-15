# rqx

A Rust-backed Python HTTP client with an httpx-compatible API.

rqx replaces httpx's pure-Python internals with a Rust core built on [`reqwest`](https://github.com/seanmonstar/reqwest) and [`tokio`](https://github.com/tokio-rs/tokio). The goal: keep the API your code already targets, but eliminate the structural performance ceilings of pure-Python HTTP under concurrent load.

## Origin

rqx is a personal learning project for PyO3 and maturin. The structure of the work was a forcing function: I wrote a normal product spec, then asked Claude to rewrite it in the form of an academic course-project spec (CS 262A — Advanced Topics in Computer Systems). The academic framing pushed the design toward sharper engineering decisions — measurable acceptance criteria, explicit architectural trade-offs, and concrete performance targets — than a casual spec would have produced.

Two documents capture this:

- **[docs/reqx_project_spec.md](docs/reqx_project_spec.md)** — the original project specification: problem statement, design constraints, acceptance criteria.
- **[docs/report.md](docs/report.md)** — the write-up: design decisions, benchmark methodology and full measurement results, lessons learned.

If you only have time for one, read the report. The benchmark numbers, the architectural trade-offs (sync vs async paths, retry placement, JSON parsing strategy, runtime singleton), and the things that didn't work all live there.

## Quick look

```python
import rqx

# Sync
with rqx.Client() as client:
    resp = client.get("https://httpbin.org/get")
    print(resp.json())

# Async
async with rqx.AsyncClient() as client:
    resp = await client.get("https://httpbin.org/get")
    print(resp.json())

# Module-level convenience (one-off requests)
resp = rqx.get("https://httpbin.org/get")

# With retries
transport = rqx.HTTPTransport(
    retries=rqx.Retry(total=3, backoff_factor=0.5, status_forcelist={503}),
)
with rqx.Client(transport=transport) as client:
    resp = client.get("https://example.com/api")
```

The API targets feature parity with [httpx](https://github.com/encode/httpx) — clients, transports, retries, streaming, mTLS, base URLs, granular timeouts, and the full exception hierarchy. See `python/rqx/_types.pyi` for the current surface.

## Installation

```bash
pip install rqx
```

To build from source (Rust toolchain + Python 3.9+ required):

```bash
git clone https://github.com/rodcochran/rqx.git
cd rqx
just setup        # uv venv + maturin develop + dev deps
just test         # full test suite
```

## Benchmarks

Measured on a paired AWS c7i.large client/server (2 vCPU each, dedicated CPU) in `us-east-1`, hitting nginx over an intra-VPC private IP. Each cell is the median of 5 runs; each (client, concurrency, run) executes in its own Python subprocess to keep clients from contaminating each other's measurements. Full methodology and raw data: [docs/launch_report.md](docs/launch_report.md).

![Throughput at concurrency=100](docs/launch_throughput.png)

![Memory at concurrency=100](docs/launch_memory.png)

![Median latency at concurrency=100](docs/launch_latency.png)

httpx is the modern successor to requests, aiohttp is the de-facto async HTTP library, and httpr is another Rust-backed alternative.

## Status

Pre-0.1.0. Usable but the API may shift in small ways before the first tagged release. Performance results in the project report. Open issues track the v0.x roadmap — anything labeled `httpx-feature-parity` is a known surface gap.

## Contributing

This started as a learning project and stayed one. Contributions are welcome — especially around the httpx-parity surface (URL/QueryParams classes, MockTransport, event hooks, full streaming surface). See open issues for the working set, particularly anything labeled `good first issue`, and [CONTRIBUTING.md](CONTRIBUTING.md) for setup, conventions, and how to run the benchmarks.

## Acknowledgements

rqx builds on the work of several excellent projects:

- **[httpx](https://github.com/encode/httpx)** — the API design this project mirrors
- **[reqwest](https://github.com/seanmonstar/reqwest)** — the Rust HTTP client powering rqx
- **[PyO3](https://github.com/PyO3/pyo3)** and **[maturin](https://github.com/PyO3/maturin)** — Rust/Python FFI and build tooling
- **[tokio](https://github.com/tokio-rs/tokio)** — async runtime

## License

MIT
