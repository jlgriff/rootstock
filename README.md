# rootstock

Run several web apps on one small Linux server, each with its own domain and HTTPS.
Rootstock checks for conflicting domains and deploys one app at a time using Docker Compose.
One shared Traefik proxy handles routing and certificates. No management daemon stays running.

## Get started

You need Rust, Docker with Compose, and a server with the
[shared proxy installed](docs/configuration.md#server-setup). A compatible local engine such as
Docker Desktop works for development.

```sh
mkdir -p apps
cp -R templates/web apps/web
cp apps/web/.env.example apps/web/.env
```

Edit `apps/web/.env`: set your domain, container image, and the port your app listens on.
The example uses Nginx to serve a default page; replace it with your own image when ready.

```sh
cargo run -- check
cargo run -- deploy web --dry-run
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- deploy web
```

Replace the SSH destination with your server. Point your domain's DNS A record at that server.
Compose pulls the image, starts the app, and waits for its healthcheck if the image defines one.
Traefik provides HTTPS. Rootstock and the build tools run on your workstation, not the server.

Provision on AWS or DigitalOcean with the optional [cloud infrastructure](docs/configuration.md#optional-cloud-infrastructure),
or use an existing Docker host. [Bootstrap the cloud backend](docs/bootstrap.md) locally or in CI;
repeat runs reuse the deployment's resources. Application configuration and deploy commands stay the same.

Copy the example into another `apps/<name>` directory to add an app. Real application inventories,
environment files, and infrastructure stack settings are gitignored.

## See what is running

```sh
cargo run -- status
cargo run -- status --json
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- observe
```

`status` shows live services, domains, internal and published ports, health, uptime, RAM, and CPU.
SSH-connected Linux hosts also show whole-host memory, available RAM, cache, and swap.
The browser console provides app links; `status --json` and MCP include the same derived URLs.
`observe` prints a local browser URL with the same information and refreshes every ten seconds.
Stop it with Ctrl-C. Both use your selected Docker host; the browser view adds no service to the VM.
See [status configuration](docs/configuration.md#live-status-and-local-dashboard) for details.
For access from other devices, [host the dashboard behind HTTPS and password protection](docs/hosted-dashboard.md).

For AI clients, the optional [MCP adapter](docs/mcp.md) provides read-only inspection and validated
deployment plans over local stdio. Build it with `cargo build --release -p rootstock-mcp`.

## Try routing locally

With a local Docker engine running:

```sh
./tests/acceptance/run.sh
```

This starts temporary example apps, proves their domains stay separate and conflicts are refused,
then removes its test resources. No public domain or cloud account needed.

## Keep the server small

Build images outside the server. Each app can be a single compiled server with its assets baked
in; it needs no extra web-server sidecar. The shared overhead is Docker, Traefik, and its restricted
Docker socket proxy. Measure idle and loaded memory before deciding how many apps fit.

## Reference and development

Reusable deployment assets live in `templates/`. Applications in `examples/` are optional and
independent of the core workspace.

- [Complete configuration reference](docs/configuration.md): CLI, app images, routing, environment,
  server setup, infrastructure settings, releases, rollback, and resource measurements.
- [Web deployment template](templates/web/compose.yml) and [infrastructure template](infra/Pulumi.example.yaml).
- [Leptos + Tailwind example](examples/leptos/README.md): a small Rust "under construction" site with a container image and healthcheck.
- [SQLite + Litestream template](docs/sqlite-litestream.md): persistent databases, off-host backups,
  and recovery before app startup.
- [API reference](docs/api.md): every MCP tool and console HTTP route, with schemas, arguments,
  response examples, and errors; [MCP setup](docs/mcp.md) covers client configuration.
- [MIT license](LICENSE).

```sh
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cd infra && npm ci && npm test
```

The CLI, SQLite test fixture, and MCP adapter use Rust. Optional Pulumi infrastructure and its mock tests use
TypeScript with Pulumi's supported SDK; Node.js runs only on your workstation or in CI.
Small shell scripts handle container and host setup. None adds a management process to the server.
The browser console uses Tailwind utilities with compiled CSS embedded in the Rust binary;
see [styling the console](docs/configuration.md#styling-the-console) before changing its classes.
