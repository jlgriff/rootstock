# Leptos placeholder

A small, server-rendered "under construction" page with Tailwind styling. One Rust process serves
HTML and its health endpoint; CSS is embedded, and the browser needs no JavaScript, WebAssembly,
fonts, or other external assets. This is an independent Cargo workspace, so its dependencies stay
out of Rootstock's CLI builds.

From the Rootstock checkout:

```sh
SITE_ADDR=127.0.0.1:3000 cargo run --release --manifest-path examples/leptos/Cargo.toml
```

Open <http://127.0.0.1:3000>. Stop the process with Ctrl-C. The server renders Leptos components
through Axum using a single-thread Tokio runtime. The page has no application state or database.

## Container and deployment

Build outside the serving VM, choosing its CPU architecture:

```sh
docker build --platform linux/amd64 --tag rootstock-leptos:local examples/leptos
docker run --rm --publish 127.0.0.1:3000:3000 rootstock-leptos:local
```

The runtime image contains only the statically linked executable and runs as an unprivileged user.
It includes its own HTTP health probe, so it needs no shell, curl, or web-server sidecar. SIGTERM
and Ctrl-C trigger graceful HTTP shutdown. Rebuild using the same local tag for subsequent previews.
BuildKit caches dependencies and compilation on the building machine, separately per target architecture.

For a public deployment, tag and push the image to your registry under an immutable release tag
or digest. Copy the [generic web template](../../templates/web/compose.yml) into an ignored application folder
and set its ignored `.env` to your image, domain, and `APP_PORT=3000`. Run Rootstock's routing
check and deploy to the selected Docker host. Follow [server setup](../../docs/configuration.md#server-setup)
for the shared proxy, DNS, and HTTPS. Nothing is built on the serving VM. Keep real domains,
registries, and credentials in ignored configuration.

## Styling and development

Change the Leptos components in `src/lib.rs` using complete Tailwind utility names. Theme values
and source scanning live in `styles/tailwind.css`. Rebuild the embedded stylesheet from the
Rootstock checkout:

```sh
./tools/styles.sh --input examples/leptos/styles/tailwind.css --output examples/leptos/src/site.css
./tools/styles.sh --input examples/leptos/styles/tailwind.css --output examples/leptos/src/site.css --check
```

Add `--watch` for continuous CSS generation. Rebuild the Rust executable or image to include CSS
changes. Ordinary Cargo and container builds use checked-in `src/site.css`; they do not download
Tailwind or need Node.js.

```sh
cargo fmt --manifest-path examples/leptos/Cargo.toml -- --check
cargo clippy --manifest-path examples/leptos/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path examples/leptos/Cargo.toml
```

## Configuration and HTTP reference

| Setting | Default | Meaning |
| --- | --- | --- |
| `SITE_ADDR` | `0.0.0.0:3000` | Numeric listening IP and port; bind to loopback for a workstation preview |
| `--healthcheck` | Off | Probe `/healthz` on the configured port, using loopback for the default wildcard address; exit zero only for HTTP 200 |

The health probe has two-second connection and I/O timeouts. Container healthchecks run every
30 seconds with a three-second timeout, five-second startup period, and three retries. Arguments
other than `--healthcheck`, invalid addresses, and bind failures exit nonzero with diagnostics on
stderr. The server prints its listening URL on startup.

| Request | Success | Response | Errors |
| --- | --- | --- | --- |
| `GET /` | `200` | `text/html; charset=utf-8`, complete page with inline CSS | Unsupported methods return `405` |
| `GET /healthz` | `200` | `text/plain; charset=utf-8`, body `ok` followed by a newline | Unsupported methods return `405` |

Neither route requires arguments or authentication. Query parameters are unused. HEAD is
supported for these paths with the same status and headers but no response body. Unknown paths
return `404`. TLS terminates at the shared proxy when deployed; the application speaks HTTP.

```sh
curl --fail http://127.0.0.1:3000/
curl --fail http://127.0.0.1:3000/healthz
SITE_ADDR=127.0.0.1:3000 examples/leptos/target/release/leptos-placeholder --healthcheck
```
