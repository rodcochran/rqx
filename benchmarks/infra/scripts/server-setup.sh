#!/usr/bin/env bash
# Runs on the bench server. Clones rqx, starts nginx + delay-server via the
# existing docker-compose stack, and verifies they're responding. Idempotent —
# safe to re-run if the docker stack is already up.
set -euo pipefail

echo "[server-setup] waiting for cloud-init to finish (apt installs etc)..."
# Tolerate a failed cloud-init — downstream checks (docker, curl) will
# surface the real issue if anything's actually broken.
sudo cloud-init status --wait >/dev/null || echo "[server-setup] cloud-init reported failure; continuing anyway"

cd "$HOME"
if [ ! -d rqx ]; then
    git clone --depth 1 https://github.com/rodcochran/rqx.git
fi

cd "$HOME/rqx/benchmarks"
# sudo because the ubuntu user's group membership for `docker` isn't always
# active in the SSH session that runs this script — cloud-init adds ubuntu
# to the docker group, but the membership only applies to sessions started
# after that completes, and there's a race with cloud-init final stage on
# the Noble AMI. sudo sidesteps the issue.
sudo docker compose up -d

# Wait for nginx to actually serve. Compose returning isn't enough — the
# container can take a couple seconds to bind the port.
echo "[server-setup] waiting for nginx on :8080..."
for i in $(seq 1 30); do
    if curl -sf -o /dev/null http://localhost:8080/json; then
        echo "[server-setup] nginx is up"
        break
    fi
    sleep 1
done

curl -sf http://localhost:8080/json > /dev/null || {
    echo "[server-setup] FATAL: nginx never responded on :8080"
    sudo docker compose logs nginx | tail -50
    exit 1
}

echo "[server-setup] done"
