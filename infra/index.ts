import * as pulumi from "@pulumi/pulumi";
import { getAcmeEmail, getConnection, getInfraConfig } from "./config";
import { createHost } from "./infrastructure";
import { installTraefik } from "./traefik";

const config = getInfraConfig();
const host = createHost(config);
const connection = getConnection(host.publicIp, config.cloud);
const proxy = installTraefik({
  connection, hostId: host.id, acmeEmail: getAcmeEmail(), dependsOn: host.dependencies,
});

export const cloud = config.cloud;
export const hostName = config.name;
export const hostId = host.id;
export const publicIp = host.publicIp;
export const dockerHost = pulumi.interpolate`ssh://${connection.user}@${host.publicIp}`;
export const proxyStarted = proxy.id;
