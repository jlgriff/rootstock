import * as pulumi from "@pulumi/pulumi";
import { types } from "@pulumi/command";

const config = new pulumi.Config();

export type ConnectionArgs = types.input.remote.ConnectionArgs;

interface SharedConfig {
  name: string;
  sshCidr: string;
}

export type InfraConfig = SharedConfig & (
  | { cloud: "digitalocean"; region: string; size: string; sshKeyFingerprints: string[]; projectId?: string }
  | { cloud: "aws"; instanceType: string; keyName: string; rootVolumeSize: number }
);

/** Loads only the settings required by the selected cloud. */
export function getInfraConfig(): InfraConfig {
  const cloud = config.get("cloud") ?? "digitalocean";
  const sshCidr = config.get("sshCidr") ?? "0.0.0.0/0";
  if (cloud === "digitalocean") {
    const settings = new pulumi.Config("do");
    return {
      cloud, sshCidr,
      name: config.get("hostName") ?? settings.require("dropletName"),
      region: settings.require("region"),
      size: settings.require("dropletSize"),
      sshKeyFingerprints: settings.require("sshKeyFingerprints").split(",").map(value => value.trim()),
      projectId: settings.get("projectId"),
    };
  }
  if (cloud === "aws") {
    const settings = new pulumi.Config("ec2");
    return {
      cloud, sshCidr,
      name: config.require("hostName"),
      instanceType: settings.require("instanceType"),
      keyName: settings.require("keyName"),
      rootVolumeSize: settings.getNumber("rootVolumeSize") ?? 20,
    };
  }
  throw new Error(`Unsupported cloud: ${cloud}; choose aws or digitalocean`);
}

/** Authenticates through the local SSH agent or a configured private key. */
export function getConnection(host: pulumi.Input<string>, cloud: InfraConfig["cloud"]): ConnectionArgs {
  const sshAgent = config.getBoolean("sshAgent") ?? false;
  if (sshAgent && !process.env.SSH_AUTH_SOCK) {
    throw new Error("sshAgent requires SSH_AUTH_SOCK; start an SSH agent and load the host key");
  }
  return {
    host,
    user: config.get("sshUsername") ?? (cloud === "aws" ? "ubuntu" : "root"),
    ...(sshAgent ? { agentSocketPath: process.env.SSH_AUTH_SOCK } : {
      privateKey: config.requireSecret("sshPrivateKey"),
      privateKeyPassword: config.getSecret("sshPrivateKeyPassphrase"),
    }),
    dialErrorLimit: 30,
  };
}

/** Supplies the contact address for ACME account registration. */
export function getAcmeEmail(): string {
  return config.require("acmeEmail");
}
