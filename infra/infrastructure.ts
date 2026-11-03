import * as aws from "@pulumi/aws";
import * as digitalocean from "@pulumi/digitalocean";
import * as pulumi from "@pulumi/pulumi";
import { readFileSync } from "node:fs";
import { InfraConfig } from "./config";

export interface Host {
  id: pulumi.Output<string>;
  publicIp: pulumi.Output<string>;
  dependencies: pulumi.Resource[];
}

const userData = readFileSync("cloud-init.sh", "utf8");

/** Adapts provider resources to the shared SSH and Docker deployment contract. */
export function createHost(config: InfraConfig): Host {
  return config.cloud === "aws" ? createAwsHost(config) : createDigitalOceanHost(config);
}

/** Provisions the existing DigitalOcean layout without renaming its resources. */
function createDigitalOceanHost(config: Extract<InfraConfig, { cloud: "digitalocean" }>): Host {
  const droplet = new digitalocean.Droplet(config.name, {
    name: config.name, region: config.region, size: config.size,
    image: "ubuntu-24-04-x64", userData, sshKeys: config.sshKeyFingerprints,
  }, { ignoreChanges: ["image", "userData", "sshKeys"] });
  const address = new digitalocean.ReservedIp(`${config.name}-ip`, {
    dropletId: droplet.id.apply(Number), region: config.region,
  });
  const firewall = new digitalocean.Firewall(`${config.name}-fw`, {
    name: `${config.name}-fw`, dropletIds: [droplet.id.apply(Number)],
    inboundRules: [
      { protocol: "tcp", portRange: "22", sourceAddresses: [config.sshCidr] },
      { protocol: "tcp", portRange: "80", sourceAddresses: ["0.0.0.0/0", "::/0"] },
      { protocol: "tcp", portRange: "443", sourceAddresses: ["0.0.0.0/0", "::/0"] },
    ],
    outboundRules: [
      { protocol: "tcp", portRange: "1-65535", destinationAddresses: ["0.0.0.0/0", "::/0"] },
      { protocol: "udp", portRange: "1-65535", destinationAddresses: ["0.0.0.0/0", "::/0"] },
      { protocol: "icmp", portRange: "1-65535", destinationAddresses: ["0.0.0.0/0", "::/0"] },
    ],
  });
  return { id: droplet.id, publicIp: address.ipAddress, dependencies: [droplet, address, firewall] };
}

/** Provisions an IPv4 Ubuntu host without a NAT gateway or load balancer. */
function createAwsHost(config: Extract<InfraConfig, { cloud: "aws" }>): Host {
  const vpc = new aws.ec2.Vpc(`${config.name}-vpc`, {
    cidrBlock: "10.42.0.0/16", enableDnsSupport: true, enableDnsHostnames: true,
  });
  const gateway = new aws.ec2.InternetGateway(`${config.name}-gateway`, { vpcId: vpc.id });
  const subnet = new aws.ec2.Subnet(`${config.name}-subnet`, {
    vpcId: vpc.id, cidrBlock: "10.42.1.0/24", mapPublicIpOnLaunch: true,
  });
  const routes = new aws.ec2.RouteTable(`${config.name}-routes`, {
    vpcId: vpc.id, routes: [{ cidrBlock: "0.0.0.0/0", gatewayId: gateway.id }],
  });
  const association = new aws.ec2.RouteTableAssociation(`${config.name}-route-association`, {
    subnetId: subnet.id, routeTableId: routes.id,
  });
  const firewall = new aws.ec2.SecurityGroup(`${config.name}-fw`, {
    vpcId: vpc.id,
    ingress: [
      { protocol: "tcp", fromPort: 22, toPort: 22, cidrBlocks: [config.sshCidr] },
      { protocol: "tcp", fromPort: 80, toPort: 80, cidrBlocks: ["0.0.0.0/0"] },
      { protocol: "tcp", fromPort: 443, toPort: 443, cidrBlocks: ["0.0.0.0/0"] },
    ],
    egress: [{ protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] }],
  });
  const image = aws.ssm.getParameterOutput({
    name: "/aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id",
  });
  const instance = new aws.ec2.Instance(config.name, {
    ami: image.value, instanceType: config.instanceType, keyName: config.keyName,
    subnetId: subnet.id, vpcSecurityGroupIds: [firewall.id],
    associatePublicIpAddress: true, userData,
    rootBlockDevice: { volumeType: "gp3", volumeSize: config.rootVolumeSize, encrypted: true },
    metadataOptions: { httpTokens: "required" }, tags: { Name: config.name },
  }, { dependsOn: [association], ignoreChanges: ["ami", "userData", "keyName"] });
  const address = new aws.ec2.Eip(`${config.name}-ip`, {
    domain: "vpc", instance: instance.id,
  }, { dependsOn: [gateway] });
  return { id: instance.id, publicIp: address.publicIp, dependencies: [address, firewall] };
}
