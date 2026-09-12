# rqx bench infrastructure

Pulumi (TypeScript) stack plus orchestrator scripts. One command provisions a paired client and server on EC2, runs the release benches against a real remote HTTP server, copies the results to your laptop and to S3, and tears everything down. This is where the numbers in `benchmarks/<version>/report.md` come from.

## Quick start

You need: an AWS account with a CLI profile, `pulumi`, `aws`, `node`, `just`, and an SSH key pair.

```bash
just benchmarks::setup <your-aws-profile>        # once
just benchmarks::release 0.2.0 v0.2.0            # up, run, wait, archive + charts, destroy; ~95 min, ~$0.30
```

`setup` installs the Pulumi program's dependencies, creates the `dev` stack, and writes your AWS profile, your current public IP (as the SSH allow-list) and your public key into `Pulumi.dev.yaml`. That file is gitignored because it carries your IP. `Pulumi.dev.yaml.example` shows its shape if you'd rather write it by hand.

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

## The session, step by step

One script per step under `scripts/`, each short-lived and safe to rerun. The `just` recipes in `benchmarks/justfile` call them; run them directly if you prefer.

| Step | Recipe | Script |
|---|---|---|
| provision + prepare the server | `just benchmarks::up` | `scripts/up.sh` |
| build a ref, start the benches detached | `just benchmarks::run v0.2.0 [runs]` | `scripts/run.sh --ref v0.2.0 [--runs N]` |
| progress + b1 table vs the last archive | `just benchmarks::status` | `scripts/status.sh [run-id]` |
| block until finished | `just benchmarks::wait` | `scripts/wait.sh [run-id]` |
| copy down + upload to S3 | `just benchmarks::collect` | `scripts/collect.sh [run-id]` |
| collect + archive + charts | `just benchmarks::archive 0.2.0` | `scripts/collect.sh --archive 0.2.0` |
| tear down | `just benchmarks::destroy` | `scripts/destroy.sh` |
| all of the above for a release | `just benchmarks::release 0.2.0 v0.2.0` | |

- `run` clones the ref from GitHub on the client, so it benches pushed code, not your working tree. The benches run under `nohup` on the client and write to `~/results/<run-id>/`, so closing your laptop does not stop them. The run id is remembered locally; `status`, `wait` and `collect` default to it, or take one explicitly.
- `status` copies the run's directory down each time and prints the state (running, finished, failed with exit code), the current bench and run, and the b1 medians so far next to the newest `benchmarks/results/aws-*` archive via `benchmarks/compare_b1.py`.
- `collect` uploads from your laptop. An upload failure is a warning; the local copy is what matters. `--archive <version>` also copies the run to `benchmarks/results/aws-<date>-v<version>/` and renders the charts into `benchmarks/<version>/`.
- To end a run early, `destroy`. Killing your terminal only stops you watching.

Timings measured on the v0.1.5 run:

| Phase | Time |
|---|---|
| `up` | ~1 min plus ~1 min for SSH |
| `run` setup | ~4 min cold, ~1 min with a warm cargo cache |
| b1 throughput, 5 runs × 4 clients × 5 concurrencies | ~45 min |
| b2 latency, 5 runs | ~10 min |
| b8 concurrency sweep, 5 runs | ~37 min |
| total | ~95 min |

## Environment

| Variable | Default | Purpose |
|---|---|---|
| `PULUMI_STACK` | `dev` | stack to use |
| `PULUMI_BACKEND_URL` | `file://~` | Pulumi state backend |
| `PULUMI_CONFIG_PASSPHRASE` | empty | passphrase for the local backend's secrets |
| `AWS_PROFILE` | the stack's `aws:profile` | profile for pulumi and the S3 upload |
| `SSH_KEY` | `~/.ssh/id_ed25519` | private key for SSH and scp |

## Recipes

**A/B on the same instances.** Bench ref A, then ref B on the same box so hardware variance cancels:

```bash
just benchmarks::up
just benchmarks::run A 3 && just benchmarks::wait && just benchmarks::collect
just benchmarks::run B 3 && just benchmarks::wait && just benchmarks::collect
just benchmarks::destroy
```

`client-setup.sh` re-fetches the ref onto the existing clone; trust `rqx_commit` in `metadata.txt`, not `rqx_branch`.

**Setup failed on the client.** The setup scripts are piped over SSH from your checkout, so fix them locally and rerun `run`. No push needed, and the cargo cache from the failed attempt makes the rebuild about a minute.

**A run died.** `status` says "stopped without finishing" and shows the tail of `driver.log`. `collect` still copies whatever finished.

## Teardown

```bash
just benchmarks::destroy
```

Kills any run in progress with the instances. The results bucket has `forceDestroy: false`, so Pulumi reports an error for it (`BucketNotEmpty`); `destroy` says so and exits cleanly, every other resource is removed. To delete the bucket too, empty it first with `aws s3 rm --recursive s3://rqx-bench-results-<accountId>/`.

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
    ├── common.sh             # shared by the six below (paths, env defaults, ssh, run id)
    ├── up.sh                 # pulumi up + server setup
    ├── run.sh                # client setup + start run-benches.sh detached
    ├── status.sh             # copy down + progress + b1 table
    ├── wait.sh               # poll until finished
    ├── collect.sh            # copy down + S3, optional archive + charts
    ├── destroy.sh            # pulumi destroy
    ├── server-setup.sh       # remote: nginx + delay-server via docker compose
    ├── client-setup.sh       # remote: rust + uv + build rqx + patch benches
    └── run-benches.sh        # remote: b1/b2/b8 × N runs into ~/results/<run-id>/
```

## Gotchas

- **Your IP changed.** `sshAllowedCidr` is a `/32`. On a different network or VPN, SSH hangs. Fix: rerun `just benchmarks::setup <profile>`, then `just benchmarks::up`.
- **The client's tool list is explicit.** `client-setup.sh` installs `maturin`, `httpx`, `aiohttp` and `httpr` by name. It does not use the project's dependency groups, so a change to `pyproject.toml` groups does not reach the bench client.
- **Comparison clients are unpinned.** They're installed from PyPI at setup time and recorded in `metadata.txt`. Check those versions before crediting a delta between runs to rqx; httpr changed its threading model between 0.4 and 0.7.
- **Same-VPC RTT is sub-millisecond**, so every number is client-CPU-bound. See the methodology section in `benchmarks/README.md`.
- **Bench scripts hit the private IP.** Traffic stays inside the VPC. The public IP would route through the IGW and inflate latency.
