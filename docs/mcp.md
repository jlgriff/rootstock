# MCP integration

Use the optional `rootstock-mcp` binary to let an MCP client inspect deployments and request
deployment plans. It runs on the client machine over stdio, uses the existing Docker connection,
and installs no agent or listener on the serving VM.

## Build and connect

```sh
cargo build --release -p rootstock-mcp
```

Configure your MCP client to launch the absolute path to `target/release/rootstock-mcp`.
Alternatively, `cargo install --path mcp --locked` installs it in Cargo's binary directory.
Use these launch settings in your client's configuration format:

```json
{
  "command": "/opt/rootstock/bin/rootstock-mcp",
  "args": ["--apps", "/srv/rootstock/apps"],
  "env": {
    "DOCKER_HOST": "ssh://deploy@host.example.com"
  }
}
```

The paths refer to the client machine. Replace them with the installed executable and your local
inventory. Replace the SSH destination with your Docker host. For local OrbStack or another local
engine, omit `DOCKER_HOST` and use its Docker context. The MCP client must provide the same Docker
CLI, Compose plugin, environment, and SSH access you use for ordinary Rootstock commands.

This example describes launch settings, not a universal MCP client configuration wrapper. Follow
your client's configuration format. Keep actual client settings outside the checkout or under
ignored `apps/`, such as `apps/mcp-client.json`; commit no real hosts, paths, credentials, or inventory.
Rootstock does not load that client configuration itself.

## Configuration

| Setting | Default | Meaning |
| --- | --- | --- |
| `--apps <directory>` | `apps` | Inventory directory on the client; relative paths use the process working directory; absolute paths are recommended |
| `--help`, `--version` | — | Print CLI information and exit; do not include these when a client starts a session |
| `DOCKER_HOST` | Docker's default | Standard Docker endpoint, including SSH destinations |
| `DOCKER_CONTEXT` | Docker's selected context | Standard context selection; an exported value takes precedence over `DOCKER_HOST` |
| Docker TLS and SSH environment | Inherited | Uses the Docker CLI's ordinary configuration and SSH authentication |

The process fixes its inventory and inherits its Docker connection at startup. Tools cannot accept
another inventory path, arbitrary filesystem path, Docker endpoint, or shell command. Docker
context files are managed by Docker; changing the selected context can affect subsequent queries.
Restart the MCP session after changing its launch environment or inventory location.

Standard output carries MCP messages only. Startup errors go to stderr; tool failures return MCP
error results. The client owns the process lifetime. Closing the transport ends the session;
in-flight blocking Docker/SSH work can delay process shutdown. There is no application-level
Docker timeout, background polling, stored metrics, HTTP transport, or additional VM service.

## Available tools

| Tool | Purpose |
| --- | --- |
| [`list_apps`](api.md#list_apps) | Read registered app names, profiles, and declared domains |
| [`get_status`](api.md#get_status) | Read live container state, ports, health, and resource samples |
| [`check_routes`](api.md#check_routes) | Validate hostname ownership across the local inventory |
| [`plan_deploy`](api.md#plan_deploy) | Describe a validated Compose deployment command without executing it |

The [API reference](api.md) documents arguments, return fields, examples, and errors for every tool.
`tools/list` publishes descriptions and generated input/output JSON Schemas, including the
application error object. The server advertises tools only; it does not provide prompts or resources.

All tools are read-only and tagged accordingly. Deployment, restart, deletion, credential access,
and arbitrary shell execution are absent. Rootstock returns selected fields rather than resolved
Compose environments, full container inspection output, or full labels. Errors may contain local
paths or Docker/SSH diagnostics; only connect clients permitted to see your deployment metadata.

## Development

```sh
cargo test -p rootstock-mcp
cargo tree -p rootstock --edges normal
```

Protocol tests launch the real stdio binary against a synthetic Docker executable and verify
discovery, schemas, selected results, conflict refusal, argument validation, and absence of
deployment execution. `cargo test --workspace` includes them. Docker calls run on blocking workers
so the single-thread async runtime can continue handling the protocol.

MCP dependencies belong to the separate workspace package. The core library's optional `api-schema`
feature derives response schemas and is enabled by the adapter; ordinary CLI builds leave it off.
Default `cargo build` still builds only the CLI. CI's Linux release artifact contains separate
`rootstock` and `rootstock-mcp` executables, so users can install either or both.

The adapter uses the official [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
with server, macro, and stdio features. HTTP and OAuth transports are not enabled.
