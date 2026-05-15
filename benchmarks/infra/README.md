# rqx bench infrastructure

Pulumi (TypeScript) stack + orchestrator scripts that provision paired EC2 instances, run rqx benchmarks against a real remote HTTP server, capture results to S3, and tear everything down. Designed to be one-command for a full bench session.

## What it builds

- 1 VPC, 1 public subnet, 1 IGW, 1 route table (single AZ for low intra-region latency).
- 2 `c7i.large` Ubuntu 24.04 instances:
  - **client** — load generator (rqx + httpx + aiohttp). SSH-only inbound.
  - **server** — nginx + delay-server via `benchmarks/docker-compose.yaml`. All inter-instance TCP allowed from the client SG; locked off from the public internet.
- EIPs on both for stable SSH endpoints.
- S3 bucket (`rqx-bench-results-<accountId>`) with public access blocked.
- IAM role + instance profile attached to the client so it can write to the bucket without access keys.

Cost: roughly $0.20/hr for both instances + cents/month for S3. Trivial.

## One-time setup

```bash
cd benchmarks/infra
npm install
pulumi stack init dev
pulumi config set aws:region us-east-1
pulumi config set aws:profile personal             # if you have multiple AWS accounts
pulumi config set sshAllowedCidr "$(curl -s https://api.ipify.org)/32"
pulumi config set sshPublicKey "$(cat ~/.ssh/id_ed25519.pub)"
```

`pulumi up` is invoked by the orchestrator script, so you don't need to run it manually unless you want to.

## Run a bench session

```bash
./scripts/bench.sh
```

That single command does:

1. `pulumi up` — provisions VPC, EC2 client + server, S3 bucket, IAM role.
2. Waits for SSH to come up on both VMs (cloud-init takes a minute or two).
3. Runs `server-setup.sh` on the server: clones rqx, starts nginx + delay-server via docker compose, verifies nginx is responding on `:8080`.
4. Runs `client-setup.sh` on the client: installs Rust + uv, clones rqx, builds the extension in release mode (the long pole — ~10-15 min on a cold cargo cache), installs httpx + aiohttp, and `sed`-patches the target benches to point at the server's private IP.
5. Runs `run-benches.sh`: drives b1 via `run_b1.sh` (per-client subprocesses, JSONL output) and runs `b2_latency` + `b8_concurrency_sweep` five times each (configurable via `--runs-per-bench`), capturing all output to `~/results/<run-id>/`. Uploads the directory to `s3://rqx-bench-results-<accountId>/<run-id>/`.
6. Mirrors `s3://...` to `./results/<run-id>/` on your laptop.
7. Prompts: destroy infra now, or leave it running for follow-up work?

## Useful flags

```bash
./scripts/bench.sh --skip-up           # infra already provisioned; just run benches
./scripts/bench.sh --skip-destroy      # leave infra up after benches (manual teardown later)
./scripts/bench.sh --runs-per-bench 3  # cut the wall time for a fast iteration
```

## Environment overrides

| Variable | Default | Purpose |
|---|---|---|
| `PULUMI_STACK` | `dev` | Pulumi stack to use |
| `AWS_PROFILE` | (active) | AWS profile (also set via `pulumi config set aws:profile`) |
| `SSH_KEY` | `~/.ssh/id_ed25519` | Private key the orchestrator uses to SSH into both VMs |

## Manual teardown

If you used `--skip-destroy` or want to clean up later:

```bash
cd benchmarks/infra
pulumi destroy
```

The S3 bucket has `forceDestroy: false` so prior runs are preserved across teardowns. To purge it entirely, empty the bucket first (`aws s3 rm --recursive s3://rqx-bench-results-<accountId>/`) or toggle `forceDestroy: true` in `index.ts`.

## Files

```
infra/
├── Pulumi.yaml          # project config
├── index.ts             # the stack (VPC, EC2, S3, IAM)
├── package.json
├── tsconfig.json
├── .gitignore
├── README.md            # this file
└── scripts/
    ├── bench.sh         # the orchestrator — run this
    ├── server-setup.sh  # remote: nginx + delay-server via docker compose
    ├── client-setup.sh  # remote: rust + uv + build rqx + patch benches
    └── run-benches.sh   # remote: b1/b2/b8 × N runs, sync to S3
```

## Gotchas

- **Source IP rotation.** `sshAllowedCidr` is a `/32` of your current public IP. If your IP rotates mid-session (different wifi, VPN flip), SSH starts failing. Fix: `pulumi config set sshAllowedCidr "$(curl -s https://api.ipify.org)/32" && pulumi up`.
- **Bench scripts hit private IP.** The orchestrator wires bench scripts to the server's *private* IP so traffic stays inside the VPC. Hitting the public IP would route through the IGW and inflate latency.
- **First run is slow.** Rust release build is ~10-15 min cold. Subsequent runs against the same VM are minutes faster — use `--skip-up` to skip provisioning between iterations.
- **Don't commit `Pulumi.dev.yaml`.** It contains your IP. Bundled `.gitignore` excludes it.
- **Bench output already covered by `.gitignore`.** The local `./results/` directory is excluded since results are also in S3.
