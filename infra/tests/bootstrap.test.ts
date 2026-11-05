import * as assert from "node:assert/strict";
import { test } from "node:test";
import type { S3Client, S3ClientConfig } from "@aws-sdk/client-s3";
import { bootstrap, readBootstrapConfig, BootstrapConfig, BootstrapDependencies } from "../bootstrap";

const config: BootstrapConfig = {
  cloud: "aws", namespace: "example-prod", bucket: "example-state-bucket", region: "us-west-2", adoptBucket: false,
};

/** Simulates cloud boundaries while exercising real bootstrap decisions and SDK commands. */
function fixture(settings = config, fault?: "public-acl" | "bucket-policy" | "wrong-region") {
  let exists = false;
  let marker: string | undefined;
  let denied = false;
  let childFails = false;
  const operations: string[] = [];
  const projects: { id: string; name: string }[] = [];
  const keys = new Map<string, string>();
  const childEnvironments: NodeJS.ProcessEnv[] = [];
  const clientConfigs: S3ClientConfig[] = [];
  const masked: string[] = [];
  /** Models supported S3 operations without making a network connection. */
  async function send(command: { constructor: { name: string }; input: Record<string, unknown> }) {
    const name = command.constructor.name;
    operations.push(name);
    if (denied) throw Object.assign(new Error("Access denied"), { $metadata: { httpStatusCode: 403 } });
    switch (name) {
      case "HeadBucketCommand":
        if (settings.cloud === "aws") assert.equal(command.input.ExpectedBucketOwner, "example-account");
        if (!exists) throw Object.assign(new Error("Missing bucket"), { $metadata: { httpStatusCode: 404 } });
        return { BucketRegion: fault === "wrong-region" ? "another-region" : settings.region };
      case "CreateBucketCommand": exists = true; return {};
      case "GetBucketLocationCommand":
        if (settings.cloud === "aws") throw new Error("GetBucketLocation is not authorized");
        return { LocationConstraint: settings.region };
      case "GetBucketAclCommand": return { Grants: [{ Permission: "FULL_CONTROL", Grantee: { Type: fault === "public-acl" ? "Group" : "CanonicalUser" } }] };
      case "GetBucketPolicyCommand":
        if (fault === "bucket-policy") return { Policy: "existing-policy" };
        throw Object.assign(new Error("No policy"), { name: "NoSuchBucketPolicy" });
      case "GetObjectCommand":
        if (!marker) throw Object.assign(new Error("Missing marker"), { name: "NoSuchKey" });
        return { Body: { transformToString: async () => marker } };
      case "PutObjectCommand": marker = String(command.input.Body); return {};
      case "PutPublicAccessBlockCommand": return {};
      default: throw new Error(`Unexpected S3 operation: ${name}`);
    }
  }
  const dependencies: BootstrapDependencies = {
    /** Records each SDK client configuration so credential scope can be checked. */
    s3(options) {
      clientConfigs.push(options);
      return {
        send,
        /** Releases no network resources for the in-memory client. */
        destroy() {},
      } as unknown as S3Client;
    },
    /** Supplies the authenticated account identity independently of bucket names. */
    async accountId() { return "example-account"; },
    /** Models DigitalOcean's account, project, assignment, and temporary-key APIs. */
    async fetch(input, options) {
      const url = new URL(String(input));
      const path = url.pathname.replace("/v2", "");
      const method = options?.method ?? "GET";
      operations.push(`${method} ${path}`);
      const body = options?.body ? JSON.parse(String(options.body)) : {};
      if (path === "/account") return Response.json({ account: { uuid: "example-account" } });
      if (path === "/projects" && method === "GET") return Response.json({ projects, links: {} });
      if (path === "/projects" && method === "POST") {
        const project = { id: "example-project", name: body.name };
        projects.push(project);
        return Response.json({ project });
      }
      if (path === "/projects/example-project") return Response.json({ project: { id: "example-project", name: settings.namespace } });
      if (path === "/projects/example-project/resources") return Response.json({ resources: [{ status: "assigned" }] });
      if (path === "/spaces/keys" && method === "POST") {
        const id = `key-${keys.size + 1}`;
        keys.set(id, body.grants[0].permission);
        return Response.json({ key: { access_key: id, secret_key: `secret-${id}` } });
      }
      if (path.startsWith("/spaces/keys/") && method === "DELETE") return new Response(null, { status: 204 });
      throw new Error(`Unexpected API request: ${method} ${path}`);
    },
    /** Checks the credentials visible to a wrapped provisioning command. */
    async run(command, environment) {
      assert.deepEqual(command, ["pulumi", "preview"]);
      childEnvironments.push(environment);
      if (childFails) throw new Error("Preview failed");
    },
    /** Records values that must be masked before commands run. */
    mask(value) { masked.push(value); },
  };
  return {
    dependencies, operations, projects, keys, childEnvironments, clientConfigs, masked,
    /** Supplies an existing bucket whose ownership must be established. */
    existingBucket(value?: string) { exists = true; marker = value; },
    /** Simulates permission failures rather than resource absence. */
    denyAccess() { denied = true; },
    /** Exercises cleanup after a provisioning command fails. */
    failChild() { childFails = true; },
  };
}

/** Requires explicit deployment identity and rejects unsupported providers before cloud calls. */
test("bootstrap validates external configuration", () => {
  assert.throws(() => readBootstrapConfig({ ROOTSTOCK_CLOUD: "other" }), /ROOTSTOCK_CLOUD/);
  const result = readBootstrapConfig({ ROOTSTOCK_CLOUD: "aws", ROOTSTOCK_NAMESPACE: config.namespace, ROOTSTOCK_STATE_BUCKET: config.bucket, ROOTSTOCK_STATE_REGION: config.region });
  assert.deepEqual(result, config);
});

for (const cloud of ["aws", "digitalocean"] as const) {
  /** Keeps fresh and repeated runs consistent across provider adapters. */
  test(`${cloud} creates a private backend once and reuses its ownership record`, async () => {
    const settings = { ...config, cloud, region: cloud === "aws" ? config.region : "nyc3", ...(cloud === "digitalocean" ? { token: "example-token" } : {}) };
    const state = fixture(settings);
    const first = await bootstrap(settings, [], state.dependencies);
    const second = await bootstrap(settings, [], state.dependencies);
    assert.deepEqual(second, first);
    assert.match(first.backendUrl, /^s3:\/\/example-state-bucket\/pulumi-state\?/);
    assert.equal(state.operations.filter(name => name === "CreateBucketCommand").length, 1);
    assert.equal(state.operations.filter(name => name === "PutObjectCommand").length, 1);
    if (cloud === "digitalocean") {
      assert.equal(first.projectId, "example-project");
      assert.equal(state.projects.length, 1);
      assert.equal(state.operations.filter(name => name.startsWith("DELETE /spaces/keys/")).length, 2);
    } else assert.ok(state.operations.includes("PutPublicAccessBlockCommand"));
  });
}

/** Prevents authentication failures from causing create attempts against existing resources. */
test("bootstrap never treats access denied as a missing bucket", async () => {
  const state = fixture();
  state.denyAccess();
  await assert.rejects(bootstrap(config, [], state.dependencies), /Access denied/);
  assert.ok(!state.operations.includes("CreateBucketCommand"));
});

/** Requires deliberate adoption and refuses resources marked for another deployment. */
test("bootstrap refuses unmarked or differently owned buckets", async () => {
  for (const marker of [undefined, JSON.stringify({ namespace: "another-deployment" })]) {
    const state = fixture();
    state.existingBucket(marker);
    await assert.rejects(bootstrap(config, [], state.dependencies), /ownership|adopt/i);
    assert.ok(!state.operations.includes("PutObjectCommand"));
  }
});

/** Allows a manually created private bucket to be adopted explicitly. */
test("bootstrap can explicitly adopt an unmarked private bucket", async () => {
  const state = fixture();
  state.existingBucket();
  await bootstrap({ ...config, adoptBucket: true }, [], state.dependencies);
  assert.equal(state.operations.filter(name => name === "CreateBucketCommand").length, 0);
  assert.equal(state.operations.filter(name => name === "PutObjectCommand").length, 1);
});

/** Keeps broad Spaces credentials out of child commands and revokes keys after failure. */
test("DigitalOcean lends only bucket-scoped credentials and always revokes them", async () => {
  const settings = { ...config, cloud: "digitalocean" as const, region: "nyc3", token: "example-token", projectId: "example-project" };
  const state = fixture(settings);
  state.failChild();
  await assert.rejects(bootstrap(settings, ["pulumi", "preview"], state.dependencies), /Preview failed/);
  assert.equal(state.keys.get("key-1"), "fullaccess");
  assert.equal(state.keys.get("key-2"), "readwrite");
  assert.equal(state.childEnvironments[0].AWS_ACCESS_KEY_ID, "key-2");
  assert.equal(state.childEnvironments[0].AWS_SECRET_ACCESS_KEY, "secret-key-2");
  assert.equal(state.childEnvironments[0].ROOTSTOCK_PROJECT_ID, "example-project");
  assert.ok(state.masked.includes("secret-key-2"));
  assert.equal(state.operations.filter(name => name.startsWith("DELETE /spaces/keys/")).length, 2);
});

/** Prevents adoption from turning an existing public bucket into a state backend. */
test("bootstrap refuses a public ACL before recording ownership", async () => {
  const state = fixture(config, "public-acl");
  state.existingBucket();
  await assert.rejects(bootstrap({ ...config, adoptBucket: true }, [], state.dependencies), /private/);
  assert.ok(!state.operations.includes("PutObjectCommand"));
});

/** Preserves the scoped-key model when an existing Spaces bucket has a policy. */
test("bootstrap refuses Spaces bucket policies and revokes its temporary key", async () => {
  const settings = { ...config, cloud: "digitalocean" as const, region: "nyc3", token: "example-token", adoptBucket: true };
  const state = fixture(settings, "bucket-policy");
  state.existingBucket();
  await assert.rejects(bootstrap(settings, [], state.dependencies), /bucket policy/);
  assert.ok(!state.operations.includes("PutObjectCommand"));
  assert.equal(state.operations.filter(name => name.startsWith("DELETE /spaces/keys/")).length, 1);
});

/** Stops region mistakes before modifying or adopting an existing bucket. */
test("bootstrap rejects a bucket in another region", async () => {
  const state = fixture(config, "wrong-region");
  state.existingBucket();
  await assert.rejects(bootstrap({ ...config, adoptBucket: true }, [], state.dependencies), /region/);
  assert.ok(!state.operations.includes("PutObjectCommand"));
});

/** Keeps project discovery from selecting an arbitrary matching project. */
test("bootstrap rejects ambiguous DigitalOcean projects", async () => {
  const settings = { ...config, cloud: "digitalocean" as const, region: "nyc3", token: "example-token" };
  const state = fixture(settings);
  state.projects.push({ id: "first-project", name: config.namespace }, { id: "second-project", name: config.namespace });
  await assert.rejects(bootstrap(settings, [], state.dependencies), /ambiguous/);
  assert.equal(state.keys.size, 0);
});
