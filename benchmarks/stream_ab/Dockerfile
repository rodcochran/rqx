# A/B harness for PR #139 (issue #108) — "remove the double copy on stream".
#
# Linux toolchain on purpose: the change removes a malloc per chunk, and
# glibc's allocator behaves differently from macOS libmalloc. Measuring on
# the host would give the right direction but a magnitude that doesn't
# transfer to the wheels we actually ship.
#
# nginx and the benchmark run in THIS container, talking over loopback. That
# is deliberate — benchmarks/nginx/nginx-host.conf documents that Docker
# Desktop's virtio networking caps out around 100 KB payloads on macOS.
# Loopback inside a single container never crosses the VM boundary.

FROM python:3.12-slim-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git nginx pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cargo.toml is edition 2024, which needs >= 1.85. Both arms build with this
# same toolchain, so the compiler is never a variable between them.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

RUN pip install --no-cache-dir "maturin>=1.12,<2.0"

# Static bodies generated at image build so both arms stream byte-identical
# payloads. /dev/urandom because incompressible data keeps Content-Encoding
# out of the measurement entirely.
RUN mkdir -p /srv/payloads \
 && head -c 8192     /dev/urandom > /srv/payloads/8kb.bin \
 && head -c 1048576  /dev/urandom > /srv/payloads/1mb.bin \
 && head -c 10485760 /dev/urandom > /srv/payloads/10mb.bin

COPY nginx.conf /etc/nginx/nginx.conf
# Both scripts run from /harness, so the directory is already on sys.path.
COPY *.py /harness/
COPY run_ab.sh       /harness/run_ab.sh
RUN chmod +x /harness/run_ab.sh

ENTRYPOINT ["/harness/run_ab.sh"]
