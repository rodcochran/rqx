# rqx bench infrastructure

Pulumi (TypeScript) stack plus orchestrator scripts. One command provisions a paired client and server on EC2, runs the release benches against a real remote HTTP server, copies the results to your laptop and to S3, and tears everything down. This is where the numbers in `benchmarks/<version>/report.md` come from.

## Quick start

You need: an AWS account with a CLI profile, `pulumi`, `aws`, `node`, and an SSH key pair.

```bash
cd benchmarks/infra
./scripts/setup.sh --profile <your-aws-profile>   # once
./scripts/bench.sh                                # ~95 min, ~$0.30
```

`setup.sh` installs the Pulumi program's dependencies, creates the `dev` stack, and writes your AWS profile, your current public IP (as the SSH allow-list) and your public key into `Pulumi.dev.yaml`. That file is gitignored because it carries your IP. `Pulumi.dev.yaml.example` shows its shape if you'd rather write it by hand.

The scripts use Pulumi's local file backend with a blank passphrase, so no Pulumi account is needed and your global `pulumi login` is untouched. Export `PULUMI_BACKEND_URL` and `PULUMI_CONFIG_PASSPHRASE` yourself to use a different backend.

## What it builds

- 1 VPC, 1 public subnet, 1 IGW, 1 route table. Single AZ so intra-region latency is as low as it gets.
- 2 EC2 instances (`c7i.large` by default), Ubuntu 24.04:
  - **client**: load generator. rqx built from source in release mode, plus httpx, aiohttp and httpr. SSH-only inbound.
  - **server**: nginx + delay-server from `benchmarks/docker-compose.yaml`. Reachable from the client's security group only.
- EIPs on both, so SSH endpoints stay stable across stop/start.
- An S3 bucket, `rqx-bench-results-<accountId>`, public access blocked. Survives `pulumi destroy`, so every run is kept.

Cost: about $0.20 per hour while the instances are up, cents per month for S3.

## Configuration

| Key | Where | Default |
|---|---|---|
| `aws:profile` | `Pulumi.dev.yaml`, written by `setup.sh` | none |
| `sshAllowedCidr` | `Pulumi.dev.yaml`, written by `setup.sh` | your IP `/32` |
| `sshPublicKey` | `Pulumi.dev.yaml`, written by `setup.sh` | `~/.ssh/id_ed25519.pub` |
| `aws:region` | `Pulumi.yaml`, override per stack | `us-east-1` |
| `instanceType` | `Pulumi.yaml`, override per stack | `c7i.large` |

Override a default with `pulumi config set aws:region us-west-2` after setup, or set it in `Pulumi.dev.yaml` directly.

## What a session does

`bench.sh` runs these steps in order:

1. `pulumi up`.
2. Waits for SSH on both instances (cloud-init takes a minute or two).
3. `server-setup.sh` on the server: clones rqx, `docker compose up` for nginx and the delay server, checks nginx answers on `:8080`.
4. `client-setup.sh` on the client: installs Rust and uv, clones the requested ref, builds the extension in release mode, installs the comparison clients, and patches the bench scripts to target the server's private IP.
5. `run-benches.sh` on the client: b1 via `run_b1.sh`, then b2 and b8, five runs each. Output lands in `~/results/<run-id>/` on the client along with `metadata.txt` (commit, toolchain, comparator versions).
6. Copies that directory to `./results/<run-id>/` with `scp`, then uploads it to the bucket from your laptop. A failed upload is a warning; the data is already on disk.
7. Asks whether to destroy the infrastructure.

Timings measured on the v0.1.5 run:

| Phase | Time |
|---|---|
| `pulumi up` | ~1 min |
| client setup | ~4 min cold, ~1 min with a warm cargo cache |
| b1 throughput, 5 runs × 4 clients × 5 concurrencies | ~45 min |
| b2 latency, 5 runs | ~10 min |
| b8 concurrency sweep, 5 runs | ~37 min |
| total | ~95 min |

## Flags and environment

```bash
./scripts/bench.sh --ref v0.2.0          # bench a tag, branch, or commit (default: main)
./scripts/bench.sh --skip-destroy        # leave the instances up; tear down later yourself
./scripts/bench.sh --skip-up             # instances already exist; skip pulumi up
./scripts/bench.sh --runs-per-bench 3    # shorter session for a quick regression check
```

`--ref` clones from GitHub on the client, so it benches pushed code, not your working tree.

| Variable | Default | Purpose |
|---|---|---|
| `PULUMI_STACK` | `dev` | stack to use |
| `PULUMI_BACKEND_URL` | `file://~` | Pulumi state backend |
| `PULUMI_CONFIG_PASSPHRASE` | empty | passphrase for the local backend's secrets |
| `AWS_PROFILE` | the stack's `aws:profile` | profile for the S3 upload |
| `SSH_KEY` | `~/.ssh/id_ed25519` | private key for SSH and scp |

## Recipes

**Release run.** Tag first, then bench the tag non-interactively and tear down when you've looked at the numbers:

```bash
./scripts/bench.sh --ref v0.2.0 --skip-destroy 2>&1 | tee bench.log
cp -R results/<run-id> ../results/aws-<date>-v<version>
python ../plot_bench.py ../results/aws-<date>-v<version>/ --out-dir ../<version>
pulumi destroy --yes
```

**A/B on the same instances.** Bench ref A, then ref B on the same box so hardware variance cancels:

```bash
./scripts/bench.sh --ref A --skip-destroy
./scripts/bench.sh --ref B --skip-up --skip-destroy
```

`client-setup.sh` re-fetches the ref onto the existing clone; trust `rqx_commit` in `metadata.txt`, not `rqx_branch`.

**Setup failed on the client.** The setup scripts are piped over SSH from your checkout, so fix them locally and relaunch with `--skip-up --skip-destroy`. No push needed, and the cargo cache from the failed attempt makes the rebuild about a minute.

**Reading results before the copy.** The driver log has every bench's stdout: b1 rows are JSON lines with `"client"`, b2 prints a percentile block per client, b8 prints `[client c=N] run k:` lines.

## Teardown

```bash
cd benchmarks/infra
pulumi destroy --yes
```

The results bucket has `forceDestroy: false`, so `destroy` reports an error for it (`BucketNotEmpty`). That is expected; every other resource is removed. To delete the bucket too, empty it first with `aws s3 rm --recursive s3://rqx-bench-results-<accountId>/`.

## Files

```
infra/
├── Pulumi.yaml               # project + shared defaults (region, instance type)
├── Pulumi.dev.yaml.example   # shape of the per-operator stack config
├── index.ts                  # the stack (VPC, EC2, S3)
├── package.json
├── tsconfig.json
├── README.md
└── scripts/
    ├── setup.sh              # once: deps + stack + your profile/IP/key
    ├── bench.sh              # the orchestrator
    ├── server-setup.sh       # remote: nginx + delay-server via docker compose
    ├── client-setup.sh       # remote: rust + uv + build rqx + patch benches
    └── run-benches.sh        # remote: b1/b2/b8 × N runs into ~/results/<run-id>/
```

## Gotchas

- **Your IP changed.** `sshAllowedCidr` is a `/32`. On a different network or VPN, SSH hangs. Fix: `pulumi config set sshAllowedCidr "$(curl -s https://api.ipify.org)/32"`, then `pulumi up` (or rerun `setup.sh`).
- **The client's tool list is explicit.** `client-setup.sh` installs `maturin`, `httpx`, `aiohttp` and `httpr` by name. It does not use the project's dependency groups, so a change to `pyproject.toml` groups does not reach the bench client.
- **Comparison clients are unpinned.** They're installed from PyPI at setup time and recorded in `metadata.txt`. Check those versions before crediting a delta between runs to rqx; httpr changed its threading model between 0.4 and 0.7.
- **Same-VPC RTT is sub-millisecond**, so every number is client-CPU-bound. See the methodology section in `benchmarks/README.md`.
- **Bench scripts hit the private IP.** Traffic stays inside the VPC. The public IP would route through the IGW and inflate latency.
