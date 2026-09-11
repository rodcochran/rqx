"""Everything under tests/integration talks to an httpbin container.

The `httpbin` fixture starts `ghcr.io/psf/httpbin` (the maintained fork with
the original response contract) through testcontainers, once per pytest run:
the first xdist worker to need it starts a container with a fixed name under
a file lock, later workers find it by name, and the controller removes it in
`pytest_unconfigure` (tests/conftest.py). Set `RQX_HTTPBIN_URL` to point at
an httpbin you run yourself and no container is started. Marked so
`-m "not integration"` can skip the whole directory.
"""

import os
import time
import urllib.request
from pathlib import Path

import filelock
import pytest

# Cleanup is explicit (controller, end of run), not Ryuk's: Ryuk would reap
# the container as soon as the worker that started it exits, while other
# workers may still be using it.
os.environ.setdefault("TESTCONTAINERS_RYUK_DISABLED", "true")

HERE = Path(__file__).parent
IMAGE = "ghcr.io/psf/httpbin:0.10.2"  # :latest arm64 image is broken (gevent)
PORT = 8080
CONTAINER_NAME = "rqx-test-httpbin"


def pytest_collection_modifyitems(items):
    for item in items:
        if HERE in item.path.parents:
            item.add_marker(pytest.mark.integration)


def _running_container_url():
    """URL of an existing, running container of ours, or None."""
    import docker
    from docker.errors import NotFound

    try:
        c = docker.from_env().containers.get(CONTAINER_NAME)
    except NotFound:
        return None
    if c.status != "running":
        c.remove(force=True)
        return None
    host_port = c.attrs["NetworkSettings"]["Ports"][f"{PORT}/tcp"][0]["HostPort"]
    return f"http://localhost:{host_port}"


def _start_container_url():
    from testcontainers.core.container import DockerContainer

    c = (
        DockerContainer(IMAGE)
        .with_name(CONTAINER_NAME)
        .with_exposed_ports(PORT)
        .start()
    )
    return f"http://{c.get_container_host_ip()}:{c.get_exposed_port(PORT)}"


def _wait_ready(url, timeout=60.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(f"{url}/get", timeout=2):
                return
        except Exception:
            time.sleep(0.1)
    raise RuntimeError(f"httpbin at {url} never became ready")


@pytest.fixture(scope="session")
def httpbin(tmp_path_factory):
    override = os.environ.get("RQX_HTTPBIN_URL")
    if override:
        return override.rstrip("/")
    # basetemp's parent is shared by every xdist worker of one run.
    lock_path = tmp_path_factory.getbasetemp().parent / "rqx-httpbin.lock"
    with filelock.FileLock(str(lock_path)):
        url = _running_container_url() or _start_container_url()
    _wait_ready(url)
    return url
