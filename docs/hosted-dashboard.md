# Host the dashboard

Run the Rootstock console on your Linux Docker host behind Traefik's HTTPS and password
protection. The console is read-only and uses the existing socket proxy. Other applications
keep their own access policies.

## Prepare the application

From the repository root:

```sh
mkdir -p apps/dashboard
cp templates/dashboard/compose.yml apps/dashboard/
cp templates/dashboard/.env.example apps/dashboard/.env
chmod 600 apps/dashboard/.env
docker build --platform linux/amd64 --tag rootstock-dashboard:deploy .
```

Use the target server's architecture. The build context excludes local inventories, environment
files, and infrastructure settings. The runtime image contains the Rust console, Docker CLI,
and Alpine; it contains no application source, Compose plugin, or SSH client.

Generate a strong password in your password manager and run `htpasswd -nBC 10 admin` on your
workstation to produce its bcrypt entry. This prompts for the password rather than putting it
in shell history. The command is provided by Apache's password utilities.

Set these values in the ignored `apps/dashboard/.env`:

| Setting | Meaning |
| --- | --- |
| `APP_IMAGE` | Locally built or preloaded dashboard image; defaults in the sample to `rootstock-dashboard:deploy` |
| `APP_DOMAIN` | Dashboard hostname, such as `dashboard.example.com`; no scheme, port, or path |
| `DASHBOARD_AUTH` | Required `username:bcrypt-hash` entry from `htpasswd`; comma-separated entries allow multiple logins |
| `ROOTSTOCK_SOCKET_NETWORK` | Existing socket-proxy network; defaults to `rootstock-proxy_socket`, from the proxy Compose project name |

Wrap the complete `DASHBOARD_AUTH` value in single quotes in `.env` to preserve the dollar signs
in bcrypt hashes. Keep the plaintext password in your password manager. The hash belongs in
ignored deployment configuration or your deployment secret store; Pulumi does not need it.
The template refuses to resolve without a nonempty authentication entry.

## Transfer and deploy

Point the dashboard's DNS A record at the server's stable IPv4 address. Publish an AAAA record
only if that IPv6 address reaches the same proxy. The shared proxy must already be running.

```sh
docker image save rootstock-dashboard:deploy |
  docker --host ssh://deploy@host.example.com image load
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- deploy dashboard
```

Open `https://dashboard.example.com` and enter the configured username and password. Traefik
obtains and renews the certificate automatically. This template uses `pull_policy: never`;
rebuild and transfer the same tag before redeploying. For registry releases, change the ignored
Compose configuration to your normal pull policy and image reference.

The backend joins only the existing private socket-proxy network and listens inside its container
on port 8787, with no published host port. Its router
requires Basic authentication for every path, including the favicon, and removes the
Authorization header before forwarding. The default `observe` command continues to bind only
to loopback. `observe --public-host <hostname>` enables the hosted listener and accepts that
hostname, optionally with `:443`, plus loopback healthcheck requests. Deploy this mode behind
an authenticated proxy; the Rust HTTP server itself does not authenticate requests.

## Inspect and maintain

The dashboard discovers Rootstock containers directly; it does not mount your application
inventories or their secrets. Its read-only filesystem, unprivileged user, and socket-proxy
connection keep it separate from deployment commands. The Docker socket is not mounted into
the console. It adds one resident console container; Docker CLI queries run transiently while
pages load. The dashboard and its temporary queries are included in resource measurements.

The template mounts the host's `/proc/meminfo` read-only and sets `ROOTSTOCK_HOST_MEMINFO` to
that file inside the container. This supplies whole-host memory without SSH keys or an agent.
Container state and host memory reuse the same collector as CLI and MCP status. Their data
contract and HTTP responses are documented in the [API reference](api.md).

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/dashboard --project-name rootstock-dashboard logs --tail 50
curl -I https://dashboard.example.com/
curl --user admin https://dashboard.example.com/
```

The anonymous request should return 401. `curl --user admin` prompts for the password; successful
page requests return 200. An authenticated HEAD request reaches the console's GET-only API and
returns 405. Rotate access by replacing the hash in the ignored `.env` and redeploying. Stop
the console with `docker compose down` using the same project and directory; the shared proxy
and application containers continue running.
