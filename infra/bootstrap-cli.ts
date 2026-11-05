import { S3Client } from "@aws-sdk/client-s3";
import { GetCallerIdentityCommand, STSClient } from "@aws-sdk/client-sts";
import { spawn } from "node:child_process";
import { appendFileSync } from "node:fs";
import { bootstrap, readBootstrapConfig } from "./bootstrap";

/** Runs provisioning with borrowed credentials and forwards cancellation to its child. */
async function run(command: string[], environment: NodeJS.ProcessEnv): Promise<void> {
  const child = spawn(command[0], command.slice(1), { env: environment, stdio: "inherit" });
  /** Lets the child stop before temporary credentials are revoked. */
  function interrupt(signal: NodeJS.Signals) { child.kill(signal); }
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  try {
    await new Promise<void>((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => code === 0 ? resolve() : reject(new Error(`Command failed (${signal ?? code})`)));
    });
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
  }
}

/** Masks generated credentials before any wrapped CI command can emit them. */
function mask(value: string): void {
  if (process.env.GITHUB_ACTIONS === "true") process.stdout.write(`::add-mask::${value.replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A")}\n`);
}

/** Exposes the same bootstrap operation to local shells and opt-in CI. */
async function main(): Promise<void> {
  if (process.argv[2] === "--help") {
    process.stdout.write("Usage: npm run bootstrap -- [command arguments...]\nRequires ROOTSTOCK_CLOUD, ROOTSTOCK_NAMESPACE, ROOTSTOCK_STATE_BUCKET, ROOTSTOCK_STATE_REGION.\nSee docs/bootstrap.md for credentials, ownership, and CI setup.\n");
    return;
  }
  const config = readBootstrapConfig(process.env);
  const result = await bootstrap(config, process.argv.slice(2), {
    /** Uses the supported SDK for S3 and Spaces requests. */
    s3(options) { return new S3Client(options); },
    /** Verifies AWS bucket ownership against the authenticated account. */
    async accountId() {
      const sts = new STSClient({ region: config.region });
      try {
        const account = (await sts.send(new GetCallerIdentityCommand({}))).Account;
        if (!account) throw new Error("AWS did not return an account ID");
        return account;
      } finally { sts.destroy(); }
    },
    fetch, run, mask,
  });
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, `backend_url=${result.backendUrl}\nproject_id=${result.projectId ?? ""}\n`);
  }
  process.stdout.write(`${JSON.stringify(result)}\n`);
}

main().catch(error => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
