import * as pulumi from "@pulumi/pulumi";
import type { MockResourceArgs } from "@pulumi/pulumi/runtime";

const resources: Pick<MockResourceArgs, "type" | "name" | "inputs">[] = [];

/** Runs the real stack against an in-memory provider monitor. */
async function main() {
  await pulumi.runtime.setMocks({
    /** Captures cloud resources without creating or contacting a cloud account. */
    newResource(args) {
      resources.push({ type: args.type, name: args.name, inputs: args.inputs });
      return {
        id: args.type === "digitalocean:index/droplet:Droplet" ? "123" : `${args.name}-id`,
        state: { ...args.inputs, publicIp: "203.0.113.10", ipAddress: "203.0.113.10", ipv4Address: "203.0.113.11", dropletUrn: "do:droplet:123" },
      };
    },
    /** Supplies a public Ubuntu image without requiring AWS credentials. */
    call(args) {
      if (args.token === "aws:ssm/getParameter:getParameter") return { ...args.inputs, value: "ami-example" };
      throw new Error(`Unexpected provider lookup: ${args.token}`);
    },
  }, "rootstock", "test");
  const stack = await import("../index");
  const [ip, dockerHost] = await new Promise<[string, string, string]>(resolve => {
    pulumi.all([stack.publicIp, stack.dockerHost, stack.proxyStarted]).apply(resolve);
  });
  process.stdout.write(JSON.stringify({ cloud: stack.cloud, ip, dockerHost, resources }));
}

main().catch(error => { process.stderr.write(error.message); process.exitCode = 1; });
