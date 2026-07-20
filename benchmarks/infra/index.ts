/**
 * Pulumi stack for rqx remote benchmarks.
 *
 * Provisions two `c7i.large` EC2 instances (client + server) in a single AZ,
 * connected over a private VPC subnet. Both also have public IPs so the
 * operator can SSH in directly; HTTP traffic between client and server is
 * restricted to the private network via security groups.
 *
 * Bench harness invocation order:
 *   1. SSH into the server, start nginx + delay-server via docker compose.
 *   2. SSH into the client, build rqx in release mode, run benches against
 *      the server's *private* IP (avoids unnecessary trips through the IGW).
 *   3. Capture results to disk on the client; `scp` back to laptop on exit.
 *   4. `pulumi destroy` to tear everything down once results are saved.
 *
 * Config keys (set via `pulumi config set`):
 *   - aws:region             — AWS region (e.g., us-east-1)
 *   - sshAllowedCidr         — CIDR allowed to SSH (e.g., "203.0.113.5/32")
 *   - sshPublicKey           — contents of your SSH public key
 *   - instanceType           — optional, defaults to c7i.large
 */

import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";

const config = new pulumi.Config();
const sshAllowedCidr = config.require("sshAllowedCidr");
const sshPublicKey = config.require("sshPublicKey");
const instanceType = config.get("instanceType") ?? "c7i.large";

// Latest Ubuntu 24.04 LTS (Noble) AMI from Canonical, x86_64.
const ubuntu = aws.ec2.getAmi({
    mostRecent: true,
    owners: ["099720109477"], // Canonical
    filters: [
        {
            name: "name",
            values: ["ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-amd64-server-*"],
        },
        { name: "architecture", values: ["x86_64"] },
        { name: "virtualization-type", values: ["hvm"] },
    ],
});

// ---------------------------------------------------------------------------
// Networking
// ---------------------------------------------------------------------------

const vpc = new aws.ec2.Vpc("rqx-bench-vpc", {
    cidrBlock: "10.0.0.0/16",
    enableDnsHostnames: true,
    enableDnsSupport: true,
    tags: { Name: "rqx-bench-vpc" },
});

const igw = new aws.ec2.InternetGateway("rqx-bench-igw", {
    vpcId: vpc.id,
    tags: { Name: "rqx-bench-igw" },
});

// Single AZ for both instances — minimizes intra-region latency variance.
const azs = aws.getAvailabilityZones({ state: "available" });

const subnet = new aws.ec2.Subnet("rqx-bench-subnet", {
    vpcId: vpc.id,
    cidrBlock: "10.0.1.0/24",
    availabilityZone: azs.then(a => a.names[0]),
    mapPublicIpOnLaunch: true,
    tags: { Name: "rqx-bench-subnet" },
});

const routeTable = new aws.ec2.RouteTable("rqx-bench-rt", {
    vpcId: vpc.id,
    routes: [{ cidrBlock: "0.0.0.0/0", gatewayId: igw.id }],
    tags: { Name: "rqx-bench-rt" },
});

new aws.ec2.RouteTableAssociation("rqx-bench-rta", {
    subnetId: subnet.id,
    routeTableId: routeTable.id,
});

// ---------------------------------------------------------------------------
// Key pair
// ---------------------------------------------------------------------------

const keyPair = new aws.ec2.KeyPair("rqx-bench-key", {
    publicKey: sshPublicKey,
});

// ---------------------------------------------------------------------------
// Security groups
// Client SG is defined first so the server SG can reference it as the source
// of HTTP traffic (private VPC only — no internet exposure on 80/443).
// ---------------------------------------------------------------------------

const clientSg = new aws.ec2.SecurityGroup("rqx-bench-client-sg", {
    vpcId: vpc.id,
    description: "rqx bench client (load generator)",
    ingress: [
        { protocol: "tcp", fromPort: 22, toPort: 22, cidrBlocks: [sshAllowedCidr] },
    ],
    egress: [
        { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
    ],
    tags: { Name: "rqx-bench-client-sg" },
});

const serverSg = new aws.ec2.SecurityGroup("rqx-bench-server-sg", {
    vpcId: vpc.id,
    description: "rqx bench server (nginx + delay-server)",
    ingress: [
        { protocol: "tcp", fromPort: 22, toPort: 22, cidrBlocks: [sshAllowedCidr] },
        // Allow all TCP traffic from the client SG only — both sides are
        // operator-controlled, so opening the full port range avoids having
        // to enumerate per-bench ports (nginx on 8080, delay-server on 8081,
        // future TLS on 443, etc.). Still locked off from the public internet.
        { protocol: "tcp", fromPort: 0, toPort: 65535, securityGroups: [clientSg.id] },
    ],
    egress: [
        { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
    ],
    tags: { Name: "rqx-bench-server-sg" },
});

// ---------------------------------------------------------------------------
// User data
// Minimal — just system-level packages. Project-specific setup (clone rqx,
// build with maturin, install httpx/aiohttp) is done interactively over SSH
// so we get fast feedback when something fails.
// ---------------------------------------------------------------------------

const clientUserData = `#!/bin/bash
set -euxo pipefail
apt-get update
# Generic python3-venv / python3-dev names — works on any Ubuntu without
# pinning to a Python minor version (Ubuntu 24.04 Noble ships 3.12 by default;
# pinning to 3.11 fails because that exact package isn't in the noble repos).
apt-get install -y \\
    build-essential pkg-config libssl-dev curl git tmux \\
    python3 python3-venv python3-dev python3-pip
`;

const serverUserData = `#!/bin/bash
set -euxo pipefail
apt-get update
apt-get install -y docker.io docker-compose-v2 git tmux curl
usermod -aG docker ubuntu
systemctl enable --now docker
`;

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Results bucket + IAM
// Bench output lands in S3 so it survives `pulumi destroy`. The bucket name
// includes the AWS account ID so it's globally unique without manual config.
// ---------------------------------------------------------------------------

const callerIdentity = aws.getCallerIdentity({});

const resultsBucket = new aws.s3.Bucket("rqx-bench-results", {
    bucket: callerIdentity.then(c => `rqx-bench-results-${c.accountId}`),
    // Keep the bucket on `pulumi destroy` so results survive teardown. Delete
    // it manually once you've harvested everything you care about. Toggle to
    // `true` only if you're sure you don't need any prior run data.
    forceDestroy: false,
    tags: { Name: "rqx-bench-results" },
});

// Block all public access — these are operator-only artifacts.
new aws.s3.BucketPublicAccessBlock("rqx-bench-results-block", {
    bucket: resultsBucket.id,
    blockPublicAcls: true,
    blockPublicPolicy: true,
    ignorePublicAcls: true,
    restrictPublicBuckets: true,
});

// IAM role the client EC2 assumes — grants PutObject + ListBucket on the
// bench results bucket and nothing else.
const clientRole = new aws.iam.Role("rqx-bench-client-role", {
    assumeRolePolicy: JSON.stringify({
        Version: "2012-10-17",
        Statement: [{
            Effect: "Allow",
            Principal: { Service: "ec2.amazonaws.com" },
            Action: "sts:AssumeRole",
        }],
    }),
    tags: { Name: "rqx-bench-client-role" },
});

new aws.iam.RolePolicy("rqx-bench-client-policy", {
    role: clientRole.id,
    policy: pulumi.all([resultsBucket.arn]).apply(([bucketArn]) =>
        JSON.stringify({
            Version: "2012-10-17",
            Statement: [
                {
                    Effect: "Allow",
                    Action: ["s3:PutObject", "s3:PutObjectAcl"],
                    Resource: `${bucketArn}/*`,
                },
                {
                    Effect: "Allow",
                    Action: ["s3:ListBucket", "s3:GetBucketLocation"],
                    Resource: bucketArn,
                },
            ],
        }),
    ),
});

const clientInstanceProfile = new aws.iam.InstanceProfile("rqx-bench-client-profile", {
    role: clientRole.name,
});

const clientInstance = new aws.ec2.Instance("rqx-bench-client", {
    ami: ubuntu.then(a => a.id),
    instanceType,
    subnetId: subnet.id,
    vpcSecurityGroupIds: [clientSg.id],
    keyName: keyPair.keyName,
    iamInstanceProfile: clientInstanceProfile.name,
    userData: clientUserData,
    rootBlockDevice: { volumeSize: 16, volumeType: "gp3" },
    tags: { Name: "rqx-bench-client" },
});

const serverInstance = new aws.ec2.Instance("rqx-bench-server", {
    ami: ubuntu.then(a => a.id),
    instanceType,
    subnetId: subnet.id,
    vpcSecurityGroupIds: [serverSg.id],
    keyName: keyPair.keyName,
    userData: serverUserData,
    rootBlockDevice: { volumeSize: 16, volumeType: "gp3" },
    tags: { Name: "rqx-bench-server" },
});

// EIPs — stable across stop/start so SSH commands in pulumi outputs don't
// rot if the instances are paused mid-bench.
const clientEip = new aws.ec2.Eip("rqx-bench-client-eip", {
    instance: clientInstance.id,
    domain: "vpc",
});

const serverEip = new aws.ec2.Eip("rqx-bench-server-eip", {
    instance: serverInstance.id,
    domain: "vpc",
});

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

export const clientPublicIp = clientEip.publicIp;
export const clientPrivateIp = clientInstance.privateIp;
export const serverPublicIp = serverEip.publicIp;
export const serverPrivateIp = serverInstance.privateIp;

// Convenience: ready-to-paste SSH commands.
export const sshClient = pulumi.interpolate`ssh ubuntu@${clientEip.publicIp}`;
export const sshServer = pulumi.interpolate`ssh ubuntu@${serverEip.publicIp}`;

// The bench scripts should target the server over the private network —
// avoids round-tripping through the IGW for intra-AZ traffic. nginx is
// exposed on host port 8080 by benchmarks/docker-compose.yaml.
export const benchTargetUrl = pulumi.interpolate`http://${serverInstance.privateIp}:8080`;

// S3 bucket for bench results. Each run writes to a timestamped prefix.
export const resultsBucketName = resultsBucket.bucket;
