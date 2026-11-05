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

interface StackOptions {
  projectId?: string;
  image?: string;
  sshAgent?: boolean;
  agentSocket?: string;
}

/** Isolates stack configuration and Pulumi runtime state for each cloud. */
function runStack(cloud: string, options: StackOptions = {}) {
  const config = {
    "rootstock:cloud": cloud,
    "rootstock:hostName": "example-host",
    "rootstock:sshPrivateKey": "example-key",
    "rootstock:sshPrivateKeyPassphrase": "example-passphrase",
    "rootstock:acmeEmail": "admin@example.com",
    "rootstock:sshCidr": "203.0.113.0/24",
    ...(options.projectId ? { "do:projectId": options.projectId } : {}),
    ...(options.image ? { "do:image": options.image } : {}),
    ...(options.sshAgent ? { "rootstock:sshAgent": "true" } : {}),
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
    encoding: "utf8",
    env: { ...process.env, SSH_AUTH_SOCK: options.agentSocket ?? "/tmp/example-agent.sock", PULUMI_CONFIG: JSON.stringify(config) },
    timeout: 30000,
  });
}

/** Protects SSH exposure and the provider-independent deployment output. */
function verifyHost(cloud: string, type: string, projectId?: string) {
  const result = runStack(cloud, { projectId });
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
  assert.ok(!stack.resources.some(resource => resource.type === "digitalocean:index/projectResources:ProjectResources"));
}

/** Keeps AWS releases independent of DigitalOcean configuration. */
test("AWS provisions a host with the shared deployment contract", () => {
  verifyHost("aws", "aws:ec2/instance:Instance", "example-project-id");
});

/** Keeps DigitalOcean releases independent of AWS configuration. */
test("DigitalOcean provisions a host with the shared deployment contract", () => {
  verifyHost("digitalocean", "digitalocean:index/droplet:Droplet");
});

/** Allows a leaner operating system without changing the application deployment contract. */
test("DigitalOcean uses the configured operating system image", () => {
  const result = runStack("digitalocean", { image: "debian-13-x64" });
  assert.equal(result.status, 0, result.stderr);
  const stack: StackResult = JSON.parse(result.stdout);
  const host = stack.resources.find(resource => resource.type === "digitalocean:index/droplet:Droplet");
  assert.equal(host?.inputs.image, "debian-13-x64");
});

/** Keeps new host resources in the selected project without managing the project itself. */
test("DigitalOcean assigns the host and reserved IP to an existing project", () => {
  const result = runStack("digitalocean", { projectId: "example-project-id" });
  assert.equal(result.status, 0, result.stderr);
  const stack: StackResult = JSON.parse(result.stdout);
  const assignments = stack.resources.filter(resource => resource.type === "digitalocean:index/projectResources:ProjectResources");
  assert.equal(assignments.length, 1);
  assert.equal(assignments[0].inputs.project, "example-project-id");
  assert.deepEqual(assignments[0].inputs.resources, ["do:droplet:123", "do:floatingip:203.0.113.10"]);
  assert.ok(!stack.resources.some(resource => resource.type === "digitalocean:index/project:Project"));
});

/** Prevents private key material from entering command resources when using the local agent. */
test("SSH agent authentication omits private keys and passphrases", () => {
  const result = runStack("digitalocean", { sshAgent: true });
  assert.equal(result.status, 0, result.stderr);
  const stack: StackResult = JSON.parse(result.stdout);
  const connections = stack.resources.filter(resource => resource.type.startsWith("command:remote:")).map(resource => resource.inputs.connection.value ?? resource.inputs.connection);
  assert.ok(connections.length > 0);
  for (const connection of connections) {
    assert.equal(connection.agentSocketPath, "/tmp/example-agent.sock");
    assert.ok(!("privateKey" in connection));
    assert.ok(!("privateKeyPassword" in connection));
  }
});

/** Reports missing local agent access before attempting provisioning commands. */
test("SSH agent authentication requires an agent socket", () => {
  const result = runStack("digitalocean", { sshAgent: true, agentSocket: "" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /SSH_AUTH_SOCK/);
});

/** Refuses unsupported providers before allocating cloud resources. */
test("unsupported providers fail explicitly", () => {
  const result = runStack("unsupported");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unsupported cloud/);
});
