# Configuration reference

## Files and private settings

| Path | Purpose | Committed? |
| --- | --- | --- |
| `templates/web/compose.yml` | Generic web application template | Yes |
| `templates/web/.env.example` | Sample image, domain, and port | Yes |
| `apps/<name>/compose.yml` | Your application's deployment definition | No |
| `apps/<name>/.env` | Image reference, domain, and Compose interpolation settings | No |
| `apps/<name>/rootstock.toml` | Enabled Compose profiles, if needed | No |
| `.env` and `.env.*` | Local environment settings at any directory level | No |
| `infra/Pulumi.yaml` | Infrastructure program definition | Yes |
| `infra/Pulumi.example.yaml` | Generic stack configuration template | Yes |
| `infra/Pulumi.aws.example.yaml` | Generic AWS stack configuration template | Yes |
| `infra/cloud-init.sh` | Shared Ubuntu/Docker bootstrap for both providers | Yes |
| `infra/Pulumi.<stack>.yaml` | Your cloud settings and encrypted Pulumi secrets | No |
| `proxy/compose.yml` | Shared production proxy | Yes |
| `proxy/compose.test.yml` | Local acceptance-test overrides | Yes |

Only `.env.example` files are exempt from the environment-file ignore rules. Keep them generic.
Put private files with other names inside `apps/`, or keep them outside this checkout. Existing
tracked files remain tracked even after an ignore rule is added; remove any private files from
tracking before committing. This does not remove earlier copies from Git history.

## CLI

Run from the repository root with `cargo run -- <command>`, or build with `cargo build --release`
and use `target/release/rootstock`. `--help` and `--version` are available.

| Command or option | Behavior |
| --- | --- |
| `--apps <directory>` | Global inventory directory; defaults to `apps` |
| `list` | Lists applications, Compose project names, and resolved domains |
| `check` | Refuses conflicting domains and invalid public routing declarations |
| `status` | Reports live app containers, domains, ports, health, uptime, restarts, RAM, and CPU |
| `status --json` | Emits the same snapshot as structured JSON |
| `observe` | Serves a local browser view that queries the selected Docker host |
| `observe --port <port>` | Local loopback HTTP port; defaults to `8787`; `0` selects an available port |
| `observe --refresh <seconds>` | Browser refresh interval; defaults to `10`; minimum `1` |
| `deploy <name>` | Validates the whole inventory, then deploys one application |
| `deploy <name> --dry-run` | Resolves configuration and prints the command without starting containers |

Applications are immediate subdirectories containing `compose.yml` or `docker-compose.yml`.
The directory name is the application name; its Compose project is `rootstock-<name>`. Names must
meet Docker Compose's project-name requirements: lowercase letters, digits, hyphens, and underscores,
starting with a letter or digit. Do not put both supported Compose filenames in one application.

Rootstock invokes `docker compose config --format json` to resolve each application. Deployment
runs `docker compose up -d --wait --remove-orphans` with the same directory, project, and profiles.
It never removes volumes. `--remove-orphans` removes services no longer in that application's
Compose definition. An empty inventory contains no applications; it is not proof of a deployment.

The optional [MCP adapter](mcp.md) is a separate binary and reuses the same inventory, validation,
status collector, and deployment planner. Its tools and the console's HTTP route have an
[API reference](api.md), including machine-readable schemas and error contracts.

### Profiles

An optional `rootstock.toml` has one supported setting:

```toml
profiles = ["prod"]
```

`profiles` defaults to an empty list. List every profile needed for that application's release.
Compose otherwise hides profiled services from routing checks as well as deployment. Other
manifest fields, such as routes or static-directory paths, do not configure rootstock.

## Application image and environment

The web deployment template uses these Compose variables:

| Variable | Required? | Meaning |
| --- | --- | --- |
| `APP_IMAGE` | Yes | Container image; use an immutable release tag or digest in production |
| `APP_DOMAIN` | Yes | One hostname you own; no scheme, path, or wildcard |
| `APP_PORT` | No | Port the server listens on inside its container; defaults to `3000` |
| `COMPOSE_PROJECT_NAME` | Automatic | Compose sets it from rootstock's `--project-name`; namespaces router and service labels |

The template's `.env.example` uses `nginx:alpine` on port `80` as a runnable demonstration. Its
floating tag is not a production release policy. Replace it with your application's versioned
image and actual port. Missing or empty required variables fail during configuration resolution.

Compose reads each application's `.env` for interpolation; exported shell variables take precedence.
These values are not automatically sent into the container. Use Compose `environment` or `env_file`
for values the application needs at runtime. Rootstock does not fetch or decrypt secrets. Compose
reads environment files on the deploying client and sends their contents to the target engine.
Do not publish resolved Compose output containing application secrets.

Build the image outside the serving VM for the target CPU architecture. The optional infrastructure
stack provisions x86-64 Linux, so its application images need `linux/amd64` support. Include the
server executable and compiled assets; no runtime build step or source checkout is required.
Bind HTTP to `0.0.0.0:<APP_PORT>`, not container loopback. TLS terminates at the shared proxy.

Define an image `HEALTHCHECK` for application readiness, for example against a `/healthz` endpoint.
Include the program the healthcheck invokes; a minimal image need not contain a shell or curl.
Without a healthcheck, Compose's `--wait` establishes only that the container is running.

The template always pulls the chosen image and restarts the container unless explicitly stopped.
It publishes no host port and has no persistent volume. Add application-owned volumes when data
must survive releases. Avoid fixed `container_name` values or globally shared volume names.
Bind mounts refer to the target engine's filesystem, not your workstation, during remote deployment.

For the default SQLite deployment pattern, use the opt-in
[SQLite + Litestream template](sqlite-litestream.md). It supplies an app-owned volume, a backup
sidecar, explicit initialization, and a restore gate. The app still owns its schema and migrations.

## Routing

Applications are public by default; Rootstock adds no browser password prompt. Applications own
their authentication and access control. Proxy password protection is an explicit per-application
opt-in: add and attach a Traefik authentication middleware in that application's ignored Compose
configuration only when wanted.

Applications must join the external `rootstock-proxy` network and opt in with `traefik.enable=true`.
The template creates router and service names from `${COMPOSE_PROJECT_NAME}-web`, so copying it for
another application cannot reuse those identifiers. Give each additional public service its own
unique router and service name.

| Label suffix | Purpose |
| --- | --- |
| `routers.<name>.rule` | Explicit hostname ownership, such as ``Host(`app.example.com`)`` |
| `routers.<name>.entrypoints` | `websecure` for production HTTPS |
| `routers.<name>.tls.certresolver` | `rootstock`, the shared ACME resolver |
| `routers.<name>.service` | Target Traefik service name |
| `services.<name>.loadbalancer.server.port` | Container's internal HTTP port |

All router/service labels start with `traefik.http.`. Multiple domains use separate matchers:

```text
Host(`app.example.com`) || Host(`www.app.example.com`)
```

Traefik rejects `Host(a, b)`. Rootstock refuses duplicate domains, including case variants and
duplicates inside one application; services that opt in without a router; and rules whose domains
cannot be enumerated, such as `HostRegexp`. Unknown hostnames have no default application route.

Rules support `&&`, `||`, and parentheses. Every alternative must be restricted by a positive
`Host` matcher. For example, `Host(...) && (Path(...) || Method(...))` is bounded, while
`Host(...) || PathPrefix(...)` is rejected because its path branch would accept any hostname.
Negation, `HostRegexp`, and TCP matchers such as `HostSNI` are unsupported. Non-host HTTP matchers
are `Path`, `PathPrefix`, `PathRegexp`, `Method`, `Header`, `HeaderRegexp`, `Query`, `QueryRegexp`,
and `ClientIP`; these restrict requests but do not establish domain ownership. Arguments must be
backtick-delimited strings or double-quoted strings with JSON-compatible escapes. Text inside an
argument is never parsed as another matcher. Rootstock conservatively claims every explicit
hostname in the rule; Traefik remains responsible for matcher-specific argument validation.

## Server setup

Choose manual setup on an existing Docker host or the optional AWS/DigitalOcean stack. Rootstock
requires no agent, Rust toolchain, application source, or Node runtime on the serving VM.

### Existing Linux server

Install Docker Engine and arrange SSH access to its socket. Replace the example destination and
email with your own values. These per-command environment variables need not be committed.

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker network create rootstock-proxy
ACME_EMAIL=admin@example.com DOCKER_HOST=ssh://deploy@host.example.com \
  docker compose -f proxy/compose.yml --project-name rootstock-proxy up -d --wait
```

Create the network only once. Permit inbound TCP ports 80 and 443, plus SSH access for deployment.
Point each domain's DNS A record at the server. Publish AAAA records only if IPv6 is configured and
reachable. The proxy redirects HTTP to HTTPS and stores ACME state in its own `acme` Docker volume;
retain and back up that volume when maintaining the server.

### Migrate an existing Ferron host

This is an explicit cutover with downtime. The infrastructure program does not stop an existing
proxy or migrate legacy application services automatically. For a fresh host, use the normal
setup above instead.

1. Save the previous deployment revision, ignored application configuration, Ferron configuration,
   service units, certificate state, and recoverable application data outside the checkout.
   Record which legacy services own each writable volume and how to start them again.
2. Prepare container images and the complete new application inventory before the cutover.
   Translate legacy routes into Compose labels and preserve application data paths deliberately.
   Run `rootstock check` and rehearsal releases against that inventory. These checks do not
   migrate data or stop legacy services.
3. Run `pulumi preview` against the existing stack. Investigate any unexpected VM, disk, or reserved
   IP replacement before applying. Updated cloud-init is ignored on an existing VM; confirm Docker
   and the Compose plugin are already installed and usable there. Keep the stable DNS address.
4. During the cutover window, stop legacy application writers before handing their state to
   replacement containers. Then release ports 80 and 443 on the server:

   ```sh
   sudo systemctl disable --now ferron
   ```

   Check `sudo ss -ltnp` for another listener before starting Traefik. Retain the old configuration
   and service files for rollback; do not uninstall Ferron during the cutover.
5. Apply the prepared infrastructure changes, or start the shared proxy manually as documented
   above, then deploy each application. Verify its real hostname, valid TLS certificate, readiness,
   and recovered state. Check that an undeclared hostname reaches no application. Preserve the
   new proxy's `acme` volume for later restarts and releases.

To roll back, stop every new application that replaced a legacy writer, then stop the new proxy:

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web stop
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  -f proxy/compose.yml --project-name rootstock-proxy stop
```

Repeat the application stop for each migrated app. On the server, restore the saved configuration
and compatible application state, restart the recorded legacy application services, and run
`sudo systemctl enable --now ferron`. Recheck all application hostnames. A proxy rollback does not
reverse schema migrations; restore a compatible data checkpoint when necessary. Never let old
and new application processes write the same state simultaneously, and retain both sets of
volumes until the chosen deployment is verified.

### Optional cloud infrastructure

Needs Node.js, npm, Pulumi, a state backend, provider credentials, and an uploaded SSH public key.
Select AWS with `rootstock:cloud=aws` or DigitalOcean with `rootstock:cloud=digitalocean` (the default
for existing stacks). Only the chosen provider's host settings are required.

For DigitalOcean, authenticate with `DIGITALOCEAN_TOKEN` or a secret `digitalocean:token` setting;
see the [provider configuration](https://www.pulumi.com/registry/packages/digitalocean/).
For AWS, use your usual AWS credentials/profile or workload identity; `aws:region` selects the region.

```sh
cd infra
npm ci
pulumi login
cp Pulumi.example.yaml Pulumi.dev.yaml
pulumi stack init dev
```

For AWS, copy `Pulumi.aws.example.yaml` instead. Each sample contains reserved example settings;
replace the SSH CIDR with your own IPv4 address/range and upload the corresponding key to that cloud.

Edit the ignored stack file with your settings. `dev` is an example stack name; use your own and
match its `Pulumi.<stack>.yaml` filename. If you already use Pulumi, select your backend and stack
instead. Do not replace an existing stack's encryption metadata when adopting the example.

```sh
pulumi config set --secret sshPrivateKey < ~/.ssh/id_ed25519
pulumi preview
pulumi up
```

For an encrypted key, also set `pulumi config set --secret sshPrivateKeyPassphrase` interactively.
Review the preview before applying. Import/reconcile existing cloud resources instead of assuming
a matching name adopts them. Stack files are ignored even when their secrets are encrypted.

| Pulumi key | Required? | Meaning |
| --- | --- | --- |
| `rootstock:cloud` | No | `aws` or `digitalocean`; defaults to `digitalocean`; other values fail before provisioning |
| `rootstock:hostName` | AWS: yes | Resource name; defaults to `do:dropletName` on DigitalOcean; changing it can replace resources |
| `rootstock:sshCidr` | No | IPv4 CIDR permitted on TCP 22; defaults to `0.0.0.0/0` for existing-stack compatibility; samples require replacing a reserved CIDR |
| `rootstock:sshUsername` | No | SSH bootstrap user; defaults to `ubuntu` on AWS and `root` on DigitalOcean; must support passwordless sudo |
| `rootstock:sshPrivateKey` | Yes, secret | Private key used by provisioning commands |
| `rootstock:sshPrivateKeyPassphrase` | Optional secret | Passphrase for the private key |
| `rootstock:acmeEmail` | Yes | ACME account contact email for hosted domains |
| `do:region` | DigitalOcean: yes | Region slug |
| `do:dropletSize` | DigitalOcean: yes | Droplet size slug; example sizing is not a measured capacity guarantee |
| `do:dropletName` | DigitalOcean: unless `hostName` is set | Existing resource name, retained for compatibility |
| `do:sshKeyFingerprints` | DigitalOcean: yes | Comma-separated uploaded key fingerprints; surrounding whitespace is trimmed |
| `aws:region` | AWS: configure here or in your AWS environment | Region used by the standard Pulumi AWS provider |
| `ec2:instanceType` | AWS: yes | x86-64 EC2 instance type, such as `t3.micro`; ARM instance types are not supported by the current AMI |
| `ec2:keyName` | AWS: yes | Existing EC2 key-pair name in the selected region |
| `ec2:rootVolumeSize` | No | AWS encrypted gp3 root disk size in GiB; defaults to `20` |

Both adapters provision Ubuntu 24.04 x86-64, run the same Docker bootstrap with a 2 GB swap file,
and install the same proxy in `/srv/rootstock/proxy`. Bootstrap uses sudo, an SSH dial error limit
of 30, and adds the SSH user to the Docker group; subsequent release commands use that account
without sudo. Cloud-init, image, and uploaded-key changes do not replace an existing VM; update
the OS separately. Proxy Compose changes do trigger a fresh `compose up`.

DigitalOcean creates a droplet, reserved IPv4 address, and firewall. SSH uses the configured IPv4
CIDR; HTTP/HTTPS and outbound traffic permit IPv4/IPv6. AWS creates an EC2 instance, encrypted EBS
root disk, security group, Elastic IP, dedicated VPC, public subnet, internet gateway, and route
table. Its network is IPv4-only; no NAT gateway, load balancer, or managed container cluster is
created. AWS allows outbound IPv4 traffic and requires IMDSv2. The Ubuntu AMI comes from
[Canonical's public SSM parameter](https://ubuntu.com/aws/docs/aws-how-to/instances/find-ubuntu-images/).

Both expose only TCP 22, 80, and 443 inbound. Public IP addresses, disks, and VMs have provider
costs; a tiny instance example is not a free-tier or capacity guarantee.

Outputs are provider-independent: `cloud`, `hostName`, `hostId`, `publicIp`, `dockerHost`, and
`proxyStarted`. `publicIp` is the stable DNS address. `dockerHost` is the SSH target for releases:

```sh
DOCKER_HOST="$(cd infra && pulumi stack output dockerHost)" cargo run -- deploy web
```

Use separate stacks when moving between providers. Selecting another provider is not a live
migration: restore application data and certificate state on the new host, validate it, then
switch DNS. Docker volumes survive container releases but are host-local and do not survive host
loss without backups. Rootstock does not provision app-specific object storage, copy volumes,
or translate storage consistency/locking semantics between providers.

`npm test` in `infra/` exercises both real stack branches with Pulumi's mock resource monitor.
It checks the shared SSH output, provider isolation, bootstrap, and SSH firewall settings without
creating resources. It does not replace an authenticated preview or a live provisioning rehearsal.

### Proxy configuration

| Setting | Meaning |
| --- | --- |
| `ACME_EMAIL` | Required Compose variable; supplied from Pulumi's `rootstock:acmeEmail` or your shell |
| `ROOTSTOCK_PROXY_NETWORK` | External Docker network name; defaults to `rootstock-proxy`; custom values must match application network definitions |
| `web` / `websecure` | Entrypoints on TCP 80 / 443, with HTTP redirected to HTTPS |
| `rootstock` | ACME resolver using HTTP-01 validation on port 80 |
| `acme` | Named volume preserving `/acme/acme.json` |

Traefik's version and the Docker socket proxy's multi-platform image digest are pinned in
`proxy/compose.yml`. Both restart unless stopped. Traefik discovers opted-in containers through the socket
proxy, which has read access to container/network metadata, events, and API version. Only that
proxy mounts the Docker socket; it has no published port and uses a separate internal network.

The acceptance harness sets `ROOTSTOCK_ACCEPTANCE_RUN` for discovery filtering and overrides
`ROOTSTOCK_PROXY_NETWORK` with a unique network. It uses a dynamic loopback HTTP port and unique
Compose projects, removes only its own resources, and does not test public TLS or certificate renewal.

## Remote releases and rollback

[Docker's SSH transport](https://docs.docker.com/engine/security/protect-access/#use-ssh-to-protect-the-docker-daemon-socket)
selects the target for every Docker call rootstock makes:

```sh
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- deploy web
```

Compose files stay on the client. The SSH user must be able to access the remote Docker socket.
`DOCKER_CONTEXT`, if exported, overrides `DOCKER_HOST`; unset it before using these examples.
Authenticate the deploying client with `docker login` for private images. No Docker API TCP port
needs to be exposed. Rootstock checks its local inventory; it does not import applications from
another checkout or deployment tool. Keep a complete inventory for each server to check conflicts.

Record the previous image reference before deployment. Restore that reference in the ignored
application `.env` and deploy again to roll back. Releases recreate containers in place and may
briefly interrupt that application. There is no automatic rollback, release history, or database
migration recovery. Changing an application directory or Compose project name creates a different
project; it does not migrate or stop an existing deployment.

```sh
curl --fail https://app.example.com/healthz
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web ps
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web logs --tail 100
```

Use the health URL your application implements; the demonstration Nginx image serves `/`.
Application logs, readiness, backups, and restore procedures remain application responsibilities.

## Live status and local dashboard

```sh
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- status
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- status --json
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- observe --port 8787 --refresh 10
```

These commands use the Docker CLI and inherit its current context, `DOCKER_HOST`, `DOCKER_CONTEXT`,
TLS settings, and SSH authentication. An exported `DOCKER_CONTEXT` overrides `DOCKER_HOST`.
Use a local engine such as OrbStack by leaving the remote settings unset and selecting its context.
No cloud-specific credentials or dashboard account are required.

Status includes containers whose `com.docker.compose.project` label starts with `rootstock-`,
including the shared proxy and projects absent from the local inventory. It groups containers by
that project and uses `com.docker.compose.service` for the service name. The browser calls these
groups deployments, including shared infrastructure. Its overview shows deployment health,
declared domains, and combined memory/CPU for running containers. Details opens the full service
inventory, with individual ports, resource samples, restart counts, and uptime. Inspection is
tracked in the page URL and stays open across automatic refreshes. Missing resource samples make
the corresponding aggregate unavailable rather than presenting an incomplete sum as a total.
Docker CPU percentages are added directly and can exceed 100% on a host with multiple CPUs.
Stopped and one-off containers remain
visible until removed. Unrelated Compose projects and unlabelled containers are
outside this view. Local app folders without containers appear as `not deployed`; if the inventory
directory is absent, live discovery still works. Status reads folder registration and optional
`rootstock.toml` settings, but does not resolve Compose files or require their environment secrets.
Only containers on the selected Docker engine are observed.

Domains come from live Traefik labels. Unsupported routing rules produce a per-service
`routing_error` while the container remains visible. `check` and `deploy` retain their strict routing
validation. Domains in a status snapshot describe declared routing; they do not prove DNS, TLS,
public reachability, or the proxy's active configuration.

The console makes derived web addresses clickable in deployment summaries and container details.
The same URLs are exposed as `services[].links` in JSON and MCP; see the shared
[browser-link contract](api.md#browser-links) for inference rules and limits. Local published web
ports work without a domain; routed localhost addresses include the proxy's published host port.

Internal ports combine Docker's exposed-port metadata, current port mappings, and Traefik's
`loadbalancer.server.port` labels. Host ports are published Docker bindings, shown with the internal
port they target. Multiple isolated apps can listen internally on `3000/tcp` while the proxy shares
host ports `80` and `443` across their domains. An app without published bindings can still be
reachable through Traefik. The view does not scan sockets or enumerate host-network listeners;
metadata alone does not prove a process is listening on a port.

Health is Docker's last recorded healthcheck status. An absent check is shown explicitly. Exited
non-restarting containers with code zero appear as completed jobs; stopped persistent services
remain stopped, and nonzero exits flag their deployment for review.
Running processes, successful jobs, and unavailable healthchecks are distinct states. Uptime
is elapsed time since the current running container's start, not its creation. Restart counts are
Docker's container restart counts and reset when a container is recreated. Resource usage comes
from `docker stats --no-stream`: memory follows Docker's Linux cache adjustment, and the displayed
limit is the container's limit or the Docker host's memory when unrestricted. It is not reserved
memory. Stopped containers and missing samples have unavailable metrics rather than zero.
Container memory does not include all host overhead.

`observe` prints `http://127.0.0.1:<port>/` and stays running on the client until Ctrl-C. Open that
URL locally. It binds only to IPv4 loopback, accepts localhost/loopback Host headers, and adds no
public route or browser password gate to deployed apps. Each page load collects a fresh snapshot;
the page refreshes after the configured interval. Collection is serial, so slow Docker or SSH calls
delay the page. Docker failures produce HTTP 503 with an error; a CLI status failure exits nonzero.
No previous snapshot is presented as current after a failed collection.

The page uses compiled Tailwind CSS and server-rendered HTML, with no JavaScript, external assets, database,
or metrics history. Responses disable browser caching. Only selected status fields reach the page
or JSON output; container environments, complete labels, mounts, and credentials are excluded.
The collector issues read-only `ps`, `inspect`, `stats`, and local `context inspect` calls. It performs no app lifecycle
actions. No dashboard container, agent, Rust runtime, or extra port is installed on the serving VM;
active queries briefly use its existing Docker/SSH infrastructure.

### Styling the console

Style the Rust-rendered HTML in `src/dashboard.rs` with Tailwind utility classes. Theme colors and
the explicit source scan live in `styles/dashboard.css`. Keep complete utility names in source;
when a state changes colors, select whole class strings so Tailwind can find every variant.

```sh
./tools/styles.sh
./tools/styles.sh --check
./tools/styles.sh --watch
```

The default command compiles and minifies `src/dashboard.css`. Commit that generated file with
class or theme changes; CI runs `--check` and rejects stale output. `--watch` rebuilds on source
changes. Because CSS is embedded at Rust compile time, rebuild and restart `observe` after a
stylesheet update; the watcher does not provide browser hot reload. Ordinary Cargo builds use the
checked-in CSS and do not run Tailwind or download build tools.

The script downloads Tailwind's standalone CLI 4.3.4 into ignored `target/tools/`, verifies the
platform-specific SHA-256 checksum, and reuses it. Asset generation supports macOS and glibc Linux
on ARM64 and x86-64; it needs a POSIX shell, curl, and `sha256sum` or `shasum`. Internet access is
needed only for the initial download. Tailwind and Node are not needed in the browser or on the
serving VM, and no npm installation is required for this styling workflow. When upgrading Tailwind,
update the pinned version and checksums in `tools/styles.sh`, then regenerate and review the CSS.

The same tool accepts `--input <stylesheet>` and `--output <generated-css>` for another Rust
application, including the [Leptos example](../examples/leptos/README.md#styling-and-development).
Paths are relative to the Rootstock checkout. The defaults are `styles/dashboard.css` and
`src/dashboard.css`; `--check` compares the selected output without modifying it, and `--watch`
passes through to Tailwind for continuous generation.

### JSON snapshot

`status --json` and MCP's `get_status` return the same
[status snapshot](api.md#status-snapshot). That shared reference defines every field, type, null
value, and sampling limitation; the HTML console renders the same collected data.

## Resource measurements

Rootstock exits after each command except the optional local `observe` session. Shared resident
services are Docker, Traefik, and the socket proxy. Applications can each be a single release-built
server with bundled assets. Avoid runtime
compilation or extra sidecars unless the workload requires them.

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker stats --no-stream
ssh deploy@host.example.com 'free -m'
```

Measure the shared idle baseline, each application's idle usage, and usage under expected load.
Container stats omit some host overhead. Leave headroom for the kernel, Docker, and traffic spikes;
choose application memory limits and VM size from measurements. No capacity numbers are assumed.
