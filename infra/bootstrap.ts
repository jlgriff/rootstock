import {
  CreateBucketCommand, GetBucketAclCommand, GetBucketLocationCommand, GetBucketPolicyCommand,
  GetObjectCommand, HeadBucketCommand, PutObjectCommand, PutPublicAccessBlockCommand,
  S3Client, S3ClientConfig, BucketLocationConstraint,
} from "@aws-sdk/client-s3";
import { randomUUID } from "node:crypto";

export interface BootstrapConfig {
  cloud: "aws" | "digitalocean";
  namespace: string;
  bucket: string;
  region: string;
  projectId?: string;
  adoptBucket: boolean;
  token?: string;
}

export interface BootstrapResult {
  backendUrl: string;
  projectId?: string;
}

export interface BootstrapDependencies {
  s3(config: S3ClientConfig): S3Client;
  accountId(): Promise<string>;
  fetch: typeof fetch;
  run(command: string[], environment: NodeJS.ProcessEnv): Promise<void>;
  mask(value: string): void;
}

/** Reads installation settings without embedding account details in source. */
export function readBootstrapConfig(environment: NodeJS.ProcessEnv): BootstrapConfig {
  const cloud = environment.ROOTSTOCK_CLOUD;
  if (cloud !== "aws" && cloud !== "digitalocean") throw new Error("ROOTSTOCK_CLOUD must be aws or digitalocean");
  const namespace = required(environment, "ROOTSTOCK_NAMESPACE");
  const bucket = required(environment, "ROOTSTOCK_STATE_BUCKET");
  const region = required(environment, "ROOTSTOCK_STATE_REGION");
  if (!/^[a-z0-9][a-z0-9-]{0,61}[a-z0-9]$/.test(namespace)) throw new Error("ROOTSTOCK_NAMESPACE must contain 2–63 lowercase letters, digits, or hyphens");
  if (!/^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$/.test(bucket)) throw new Error("ROOTSTOCK_STATE_BUCKET must contain 3–63 lowercase letters, digits, or hyphens");
  if (!/^[a-z0-9-]+$/.test(region)) throw new Error("ROOTSTOCK_STATE_REGION must be a cloud region slug");
  if (environment.ROOTSTOCK_ADOPT_STATE_BUCKET && !["true", "false"].includes(environment.ROOTSTOCK_ADOPT_STATE_BUCKET)) {
    throw new Error("ROOTSTOCK_ADOPT_STATE_BUCKET must be true or false");
  }
  return {
    cloud, namespace, bucket, region, adoptBucket: environment.ROOTSTOCK_ADOPT_STATE_BUCKET === "true",
    ...(cloud === "digitalocean" ? {
      token: required(environment, "DIGITALOCEAN_TOKEN"),
      ...(environment.ROOTSTOCK_PROJECT_ID ? { projectId: environment.ROOTSTOCK_PROJECT_ID } : {}),
    } : {}),
  };
}

/** Names missing settings without revealing their values. */
function required(environment: NodeJS.ProcessEnv, key: string): string {
  const value = environment[key];
  if (!value) throw new Error(`${key} is required`);
  return value;
}

/** Separates missing resources from permission and transport failures. */
function missing(error: unknown): boolean {
  const value = error as { name?: string; $metadata?: { httpStatusCode?: number } };
  return value.$metadata?.httpStatusCode === 404 || value.name === "NoSuchKey";
}

/** Ensures the bucket's ownership record matches this installation before any reuse. */
async function ensureBucket(config: BootstrapConfig, account: string, projectId: string | undefined, s3: S3Client): Promise<void> {
  const bucket = { Bucket: config.bucket, ...(config.cloud === "aws" ? { ExpectedBucketOwner: account } : {}) };
  let created = false;
  let actualRegion: string | undefined;
  try {
    actualRegion = (await s3.send(new HeadBucketCommand(bucket))).BucketRegion;
  } catch (error) {
    if (!missing(error)) throw error;
    await s3.send(new CreateBucketCommand({
      Bucket: config.bucket,
      ...(config.cloud === "digitalocean" ? { ACL: "private" } : config.region !== "us-east-1" ? {
        CreateBucketConfiguration: { LocationConstraint: config.region as BucketLocationConstraint },
      } : {}),
    }));
    created = true;
    actualRegion = config.region;
  }
  if (config.cloud === "digitalocean") actualRegion = (await s3.send(new GetBucketLocationCommand(bucket))).LocationConstraint;
  if (actualRegion !== config.region) throw new Error("State bucket region does not match ROOTSTOCK_STATE_REGION");
  const identity = { version: 1, cloud: config.cloud, account, namespace: config.namespace, region: config.region, ...(projectId ? { projectId } : {}) };
  const marker = { ...bucket, Key: ".rootstock/bootstrap.json" };
  let stored: Record<string, unknown> | undefined;
  try {
    const object = await s3.send(new GetObjectCommand(marker));
    stored = JSON.parse(await object.Body!.transformToString());
  } catch (error) {
    if (!missing(error)) throw error;
  }
  if (stored && !Object.entries(identity).every(([key, value]) => stored![key] === value)) {
    throw new Error("State bucket ownership belongs to another deployment; refusing to adopt it");
  }
  if (!stored && !created && !config.adoptBucket) {
    throw new Error("State bucket has no ownership record; set ROOTSTOCK_ADOPT_STATE_BUCKET=true to adopt it explicitly");
  }
  const acl = await s3.send(new GetBucketAclCommand(bucket));
  if (acl.Grants?.some(grant => grant.Grantee?.Type === "Group")) throw new Error("State bucket ACL must be private");
  if (config.cloud === "aws") {
    await s3.send(new PutPublicAccessBlockCommand({
      ...bucket,
      PublicAccessBlockConfiguration: { BlockPublicAcls: true, IgnorePublicAcls: true, BlockPublicPolicy: true, RestrictPublicBuckets: true },
    }));
  } else {
    try {
      await s3.send(new GetBucketPolicyCommand(bucket));
      throw new Error("Spaces state bucket must not have a bucket policy; use private ACLs and scoped keys");
    } catch (error) {
      if ((error as { name?: string }).name !== "NoSuchBucketPolicy") throw error;
    }
  }
  if (!stored) await s3.send(new PutObjectCommand({ ...marker, Body: JSON.stringify(identity), ContentType: "application/json" }));
}

interface Project { id: string; name: string; owner_uuid: string }
interface SpacesKey { access_key: string; secret_key: string }

/** Calls DigitalOcean without copying tokens or response bodies into errors. */
async function digitalOcean<T>(config: BootstrapConfig, dependencies: BootstrapDependencies, path: string, method = "GET", body?: unknown): Promise<T> {
  const response = await dependencies.fetch(`https://api.digitalocean.com/v2${path}`, {
    method, headers: { Authorization: `Bearer ${config.token}`, "Content-Type": "application/json" },
    ...(body ? { body: JSON.stringify(body) } : {}), signal: AbortSignal.timeout(30000),
  });
  if (!response.ok) throw new Error(`DigitalOcean ${method} ${path} failed (${response.status})`);
  return response.status === 204 ? undefined as T : await response.json() as T;
}

/** Reuses an explicit project or an unambiguous deployment name without taking ownership. */
async function ensureProject(config: BootstrapConfig, dependencies: BootstrapDependencies): Promise<Project> {
  if (config.projectId) {
    const { project } = await digitalOcean<{ project: Project }>(config, dependencies, `/projects/${encodeURIComponent(config.projectId)}`);
    return project;
  }
  const matches: Project[] = [];
  for (let page = 1; ; page++) {
    const result = await digitalOcean<{ projects: Project[]; links: { pages?: { next?: string } } }>(config, dependencies, `/projects?per_page=200&page=${page}`);
    matches.push(...result.projects.filter(project => project.name === config.namespace));
    if (!result.links.pages?.next) break;
  }
  if (matches.length > 1) throw new Error("Project name is ambiguous; set ROOTSTOCK_PROJECT_ID explicitly");
  if (matches.length === 1) return matches[0];
  const { project } = await digitalOcean<{ project: Project }>(config, dependencies, "/projects", "POST", {
    name: config.namespace, description: "Shared application infrastructure", purpose: "Web Application", environment: "Production",
  });
  return project;
}

/** Prepares a durable backend and lends scoped credentials to an optional command. */
export async function bootstrap(config: BootstrapConfig, command: string[], dependencies: BootstrapDependencies): Promise<BootstrapResult> {
  if (config.cloud === "aws") {
    const account = await dependencies.accountId();
    const s3 = dependencies.s3({ region: config.region });
    const result = { backendUrl: `s3://${config.bucket}/pulumi-state?region=${config.region}` };
    try {
      await ensureBucket(config, account, undefined, s3);
      if (command.length) await dependencies.run(command, { ...process.env, PULUMI_BACKEND_URL: result.backendUrl });
      return result;
    } finally { s3.destroy(); }
  }
  const keys: string[] = [];
  /** Records each temporary credential immediately so failures can revoke it. */
  async function createKey(permission: "fullaccess" | "readwrite"): Promise<SpacesKey> {
    const { key } = await digitalOcean<{ key: SpacesKey }>(config, dependencies, "/spaces/keys", "POST", {
      name: `rootstock-${config.namespace}-${randomUUID()}`,
      grants: [{ bucket: permission === "fullaccess" ? "" : config.bucket, permission }],
    });
    keys.push(key.access_key);
    dependencies.mask(key.access_key);
    dependencies.mask(key.secret_key);
    return key;
  }
  try {
    const project = await ensureProject(config, dependencies);
    const projectId = project.id;
    const key = await createKey("fullaccess");
    const endpoint = `https://${config.region}.digitaloceanspaces.com`;
    const s3 = dependencies.s3({
      region: "us-east-1", endpoint,
      credentials: { accessKeyId: key.access_key, secretAccessKey: key.secret_key },
      requestChecksumCalculation: "WHEN_REQUIRED", responseChecksumValidation: "WHEN_REQUIRED",
    });
    try { await ensureBucket(config, project.owner_uuid, projectId, s3); }
    finally { s3.destroy(); }
    const assignment = await digitalOcean<{ resources: { status: string }[] }>(config, dependencies, `/projects/${encodeURIComponent(projectId)}/resources`, "POST", { resources: [`do:space:${config.bucket}`] });
    if (assignment.resources.length !== 1 || !["assigned", "already_assigned", "ok"].includes(assignment.resources[0].status)) {
      throw new Error("Could not assign state bucket to the selected project");
    }
    await digitalOcean(config, dependencies, `/spaces/keys/${encodeURIComponent(key.access_key)}`, "DELETE");
    keys.splice(keys.indexOf(key.access_key), 1);
    const result = { backendUrl: `s3://${config.bucket}/pulumi-state?endpoint=${endpoint}&region=us-east-1`, projectId };
    if (command.length) {
      const scoped = await createKey("readwrite");
      await dependencies.run(command, {
        ...process.env, PULUMI_BACKEND_URL: result.backendUrl, ROOTSTOCK_PROJECT_ID: projectId,
        AWS_ACCESS_KEY_ID: scoped.access_key, AWS_SECRET_ACCESS_KEY: scoped.secret_key,
        AWS_SESSION_TOKEN: undefined, AWS_PROFILE: undefined, AWS_REGION: "us-east-1",
        AWS_REQUEST_CHECKSUM_CALCULATION: "WHEN_REQUIRED", AWS_RESPONSE_CHECKSUM_VALIDATION: "WHEN_REQUIRED",
      });
    }
    return result;
  } finally {
    const failures: string[] = [];
    for (const key of keys) {
      try { await digitalOcean(config, dependencies, `/spaces/keys/${encodeURIComponent(key)}`, "DELETE"); }
      catch { failures.push(key); }
    }
    if (failures.length) throw new Error(`Temporary Spaces keys could not be revoked; remove them in the console: ${failures.join(", ")}`);
  }
}
