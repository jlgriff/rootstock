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
cp -R examples/web apps/web
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
or use an existing Docker host. Application configuration and deploy commands stay the same.

Copy the example into another `apps/<name>` directory to add an app. Real application inventories,
environment files, and infrastructure stack settings are gitignored.

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

- [Complete configuration reference](docs/configuration.md): CLI, app images, routing, environment,
  server setup, infrastructure settings, releases, rollback, and resource measurements.
- [Example application](examples/web/compose.yml) and [infrastructure template](infra/Pulumi.example.yaml).
- [SQLite + Litestream template](docs/sqlite-litestream.md): persistent databases, off-host backups,
  and recovery before app startup.
- [MIT license](LICENSE).

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cd infra && npm ci && npm run typecheck && npm test
```
