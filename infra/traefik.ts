import * as command from "@pulumi/command";
import * as pulumi from "@pulumi/pulumi";
import { readFileSync } from "node:fs";
import { ConnectionArgs } from "./config";

interface TraefikArgs {
  connection: ConnectionArgs;
  hostId: pulumi.Input<string>;
  acmeEmail: string;
  dependsOn?: pulumi.Resource[];
}

/** Where the shared proxy's Compose project lives on the VM. */
const PROXY_DIR = "/srv/rootstock/proxy";

/** The network applications join to become reachable, created once and shared. */
const PROXY_NETWORK = "rootstock-proxy";

/** Installs the shared proxy independently of application releases. */
export function installTraefik(args: TraefikArgs): command.remote.Command {
  const compose = readFileSync("../proxy/compose.yml", "utf8");
  const waitForCloudInit = new command.remote.Command(
    "wait-for-cloud-init",
    {
      connection: args.connection,
      create: `sudo cloud-init status --wait && sudo mkdir -p ${PROXY_DIR} && sudo chown $(id -un) ${PROXY_DIR}`,
      triggers: [args.hostId, PROXY_DIR],
    },
    { dependsOn: args.dependsOn },
  );

  // Applications attach to this network, so it has to outlive any one of them.
  const network = new command.remote.Command(
    "create-proxy-network",
    {
      connection: args.connection,
      create: `sudo docker network inspect ${PROXY_NETWORK} >/dev/null 2>&1 || sudo docker network create ${PROXY_NETWORK}`,
      triggers: [args.hostId],
    },
    { dependsOn: [waitForCloudInit] },
  );

  const copyCompose = new command.remote.CopyToRemote(
    "copy-proxy-compose",
    {
      connection: args.connection,
      source: new pulumi.asset.StringAsset(compose),
      remotePath: `${PROXY_DIR}/compose.yml`,
      triggers: [args.hostId],
    },
    { dependsOn: [waitForCloudInit] },
  );

  const environment = `ACME_EMAIL=${args.acmeEmail}\n`;
  const copyEnv = new command.remote.CopyToRemote(
    "copy-proxy-env",
    {
      connection: args.connection,
      source: new pulumi.asset.StringAsset(environment),
      remotePath: `${PROXY_DIR}/.env`,
      triggers: [args.hostId, environment],
    },
    { dependsOn: [waitForCloudInit] },
  );

  return new command.remote.Command(
    "start-proxy",
    {
      connection: args.connection,
      create: `set -e
cd ${PROXY_DIR}
sudo docker compose --project-name rootstock-proxy config >/dev/null
sudo docker compose --project-name rootstock-proxy up -d --wait
sudo usermod -aG docker $(id -un)`,
      triggers: [args.hostId, environment, compose],
    },
    { dependsOn: [network, copyCompose, copyEnv] },
  );
}
