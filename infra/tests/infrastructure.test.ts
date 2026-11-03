import * as assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import type { MockResourceArgs } from "@pulumi/pulumi/runtime";

interface StackResult {
  cloud: string;
  ip: string;
  dockerHost: string;
  resources: Pick<MockResourceArgs, "type" | "inputs">[];
}

/** Isolates stack configuration and Pulumi runtime state for each cloud. */
function runStack(cloud: string) {
  const config = {
    "rootstock:cloud": cloud,
    "rootstock:hostName": "example-host",
    "rootstock:sshPrivateKey": "example-key",
    "rootstock:sshPrivateKeyPassphrase": "example-passphrase",
    "rootstock:acmeEmail": "admin@example.com",
    "rootstock:sshCidr": "203.0.113.0/24",
    ...(cloud === "aws" ? {
      "aws:region": "us-east-1",
      "ec2:instanceType": "t3.micro",
      "ec2:keyName": "example-key",
    } : cloud === "digitalocean" ? {
      "do:region": "nyc1",
      "do:dropletName": "example-host",
      "do:dropletSize": "s-1vcpu-1gb",
      "do:sshKeyFingerprints": "example-fingerprint",
      "rootstock:sshUsername": "root",
    } : {}),
  };
  return spawnSync(process.execPath, ["bin/tests/stack.js"], {
    encoding: "utf8", env: { ...process.env, PULUMI_CONFIG: JSON.stringify(config) }, timeout: 30000,
  });
}

/** Protects SSH exposure and the provider-independent deployment output. */
function verifyHost(cloud: string, type: string) {
  const result = runStack(cloud);
  assert.equal(result.status, 0, result.stderr);
  const stack: StackResult = JSON.parse(result.stdout);
  assert.equal(stack.cloud, cloud);
  assert.equal(stack.ip, "203.0.113.10");
  assert.equal(stack.dockerHost, `ssh://${cloud === "aws" ? "ubuntu" : "root"}@203.0.113.10`);
  const host = stack.resources.find(resource => resource.type === type);
  assert.ok(host, `missing ${type}`);
  assert.match(host.inputs.userData, /docker-compose-plugin/);
  assert.ok(stack.resources.some(resource => resource.type === "command:remote:Command"));
  const firewall = stack.resources.find(resource => /SecurityGroup|Firewall/.test(resource.type));
  assert.ok(firewall, "missing host firewall");
  const rules = firewall.inputs.ingress || firewall.inputs.inboundRules;
  const ssh = rules.find((rule: { fromPort?: number; portRange?: string }) => rule.fromPort === 22 || rule.portRange === "22");
  assert.deepEqual(ssh.cidrBlocks || ssh.sourceAddresses, ["203.0.113.0/24"]);
  assert.ok(!stack.resources.some(resource => resource.type.startsWith(cloud === "aws" ? "digitalocean:" : "aws:")));
}

/** Keeps AWS releases independent of DigitalOcean configuration. */
test("AWS provisions a host with the shared deployment contract", () => {
  verifyHost("aws", "aws:ec2/instance:Instance");
});

/** Keeps DigitalOcean releases independent of AWS configuration. */
test("DigitalOcean provisions a host with the shared deployment contract", () => {
  verifyHost("digitalocean", "digitalocean:index/droplet:Droplet");
});

/** Refuses unsupported providers before allocating cloud resources. */
test("unsupported providers fail explicitly", () => {
  const result = runStack("unsupported");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unsupported cloud/);
});
