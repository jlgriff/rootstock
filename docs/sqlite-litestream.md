# SQLite + Litestream

Use `examples/sqlite-litestream/` for an application that keeps SQLite on local disk and replicates
it to off-host object storage. Each application gets its own database volume and one Litestream
process. Rootstock's CLI remains unaware of database schemas.

## Set up an application

Build the backup helper outside the serving VM. It contains a digest-pinned Litestream 0.5.17,
generic configuration, and the database preparation script. No application code or credentials
belong in this image. The build context allowlist excludes other files.

```sh
docker build --platform linux/amd64 \
  -t registry.example.com/team/sqlite-backup:release examples/sqlite-litestream
docker push registry.example.com/team/sqlite-backup:release
mkdir -p apps
cp -R examples/sqlite-litestream apps/web
cp apps/web/.env.example apps/web/.env
```

Use a new destination directory. To adopt this pattern for an existing application, merge the
template into its private Compose file, match the existing database path and UID/GID, and set
`SQLITE_DATA_VOLUME` to its existing volume. Do not initialize an existing database.

Replace the example registry with your own. Select the server's CPU architecture; the helper's
upstream base supports AMD64 and ARM64. All applications can reuse the same helper image.
Alternatively, deliver the built image with `docker save`/`docker load` and change the copied
Compose file's pull policy accordingly. Nothing is built on the serving VM.

Edit the ignored `.env`: choose your application image, domain, internal HTTP port, runtime UID/GID,
and your existing private backup bucket. Give every application/environment/database a distinct
backup prefix. Set credentials in that ignored file or inject them through the deploying environment.

The application image must read `DATABASE_PATH`, or you must adapt that environment variable in
your private Compose file to its own setting. The app must refuse to start when its database is
missing, including during Docker's automatic restarts. Prefer connections that open an existing
file without creating a missing one (`mode=rw`, or the driver's equivalent). For apps that open
one connection at startup, a private entrypoint can check the file exists before executing the
app. Healthchecks must never create the database. The explicit initialization command creates it.
The app must create/migrate its schema before reporting healthy. Use the runtime UID/GID the
image supports; the template runs the application and backup process as that same account.
Initial preparation sets the volume owner and private file permissions.

## Fresh initialization

For a genuinely new database with a new, unused backup prefix, initialize it explicitly:

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web \
  run --rm --no-deps prepare initialize
DOCKER_HOST=ssh://deploy@host.example.com cargo run -- deploy web
```

Always use the same Compose project name as rootstock: `rootstock-<application directory name>`.
The initialization command creates an empty, private SQLite file; the app creates its schema.
It refuses an existing database or SQLite journal files. It does not create the backup bucket or
check that a chosen prefix is unused; choose the prefix deliberately before initializing.

`initialize` and `restore` are explicit command overrides for the one-off preparation container.
Normal `rootstock deploy` always uses `prepare`; there is no initialization service that a Compose
profile could accidentally activate.

## Startup and shutdown

Normal deployment runs a short-lived `prepare` service before either the application or backup:

1. If the database file exists, preparation leaves it alone.
2. If it is missing, preparation restores from the configured backup into a temporary directory
   on the same volume. A full SQLite integrity check must pass.
3. The recovered database is published without overwriting an existing path. Failed recovery
   cleans up its temporary directory and blocks startup; no empty database is substituted.
4. Litestream starts, then the application starts and performs its own migrations.

Only the application and Litestream remain running. The completed preparation container consumes
no resident process memory. `prepare` does not integrity-check existing databases on every restart;
the application must detect unusable state through its readiness check.

Docker's automatic container restart policy does not rerun Compose preparation. The startup
requirement prevents those restarts from silently creating a new database. If the file is missing,
run `rootstock deploy` to perform recovery before restarting the app. Images that always create
missing databases do not meet this template's contract; add a startup guard or disable automatic
restarts until the app can refuse a missing database.

Compose stops the application before Litestream. Litestream's final sync has its upstream 30-second
timeout, with a 45-second container shutdown grace period. This ordering applies when stopping the
whole project, for example `docker compose ... stop`. An application-only image replacement leaves
the backup running. Do not stop the backup first while application writes continue.

Litestream runs without public ports. A private Unix socket supports operator sync commands; no
separate management service or host agent is added. Replication retries do not make the app's readiness a
backup-health check. Monitor backup logs and verify remote recovery separately.

## Recovery and restore drills

Recover into a fresh volume, preserving the old volume for investigation. First stop the original
application and backup; only one writer may use a given replica prefix.

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web stop
```

Set `SQLITE_DATA_VOLUME` in the application's ignored `.env` to a new, unique volume name, then
deploy normally. Preparation automatically restores the missing database. Compose creates the
new local volume. Inspect application readiness and recovered business data before accepting traffic.

To restore without starting the application, after selecting that fresh volume:

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web \
  run --rm --no-deps prepare restore
```

The restore command refuses an existing database, including an empty initialized file, and refuses
orphaned `-wal`, `-shm`, or `-journal` state. It never uses Litestream's force-overwrite option or
flags that silently succeed when no backup is found. Keep the old volume until recovery is verified;
do not use `down --volumes` against production data.

For a live restore drill, restore into a fresh volume with the maintenance command while the source
continues running, inspect the restored data with an isolated tool, then remove only the drill volume.
Do not start another backup writer against the source's replica prefix. A coordinated cutover requires
stopping the original writer before starting its replacement.

To request a final remote sync while the backup is running:

```sh
DOCKER_HOST=ssh://deploy@host.example.com docker compose \
  --project-directory apps/web --project-name rootstock-web \
  exec backup litestream sync -socket /tmp/litestream.sock -wait /data/app.db
```

Use your configured filename. Replication is asynchronous; a sudden host failure can lose writes
that have not reached remote storage. A container image rollback does not undo schema migrations.
Choose restore points and application versions together when recovering from a bad migration.

## Configuration

| Variable | Default / requirement | Purpose |
| --- | --- | --- |
| `APP_IMAGE` | Required | Your versioned SQLite-aware application image |
| `APP_DOMAIN` | Required | Public hostname you own |
| `APP_PORT` | `3000` | Internal HTTP port |
| `SQLITE_BACKUP_IMAGE` | Required | Built helper image; use an immutable tag or digest |
| `SQLITE_DATABASE` | `app.db` | Filename under `/data`; letters, digits, dots, underscores, and hyphens only |
| `SQLITE_UID` / `SQLITE_GID` | `1000` / `1000` | Shared runtime account for app and backup; preparation runs briefly as root |
| `SQLITE_DATA_VOLUME` | `<Compose project>-data` | Optional volume name for recovery or an existing app-owned volume |
| `LITESTREAM_BUCKET` | Required | Existing backup bucket |
| `LITESTREAM_PREFIX` | Required | Unique object prefix for this database and environment |
| `LITESTREAM_REGION` | `us-east-1` | Object-storage signing region |
| `LITESTREAM_ENDPOINT` | Empty | Empty for AWS S3; HTTPS endpoint for an S3-compatible provider |
| `LITESTREAM_RETENTION` | `168h` | Snapshot retention; other compaction/restore granularity settings retain upstream defaults |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` | Optional | Backup credentials; both are needed when using explicit access keys |
| `AWS_SESSION_TOKEN` | Empty | Token for temporary AWS credentials |

Credentials are supplied only to the backup and maintenance containers, not to the application.
If access keys are omitted, Litestream's standard AWS credential chain applies; the host templates
do not automatically grant an instance role or bucket access, and workstation profiles are not
forwarded into containers. The helper's only image configuration
is `/etc/litestream.yml`; real settings stay in ignored configuration or environment variables.

The image's default snapshot interval is 24 hours. Metrics and MCP listeners are disabled. The
control socket is `/tmp/litestream.sock`, accessible to the backup's runtime user. Stop the project
and review ownership before changing UID/GID on an existing volume. Add other per-app environment
variables, secrets, volumes, and healthchecks in your private Compose file.

### AWS and DigitalOcean

Compute and backup storage may use different providers. Use AWS S3 with an empty endpoint and the
bucket's region. For Spaces, set `LITESTREAM_ENDPOINT=https://nyc3.digitaloceanspaces.com` and
`LITESTREAM_REGION=nyc3`, replacing the example region as appropriate. Bucket and key details remain
private. Rootstock does not provision these buckets or change their access policies.

Keep buckets private, scope credentials to the intended database prefix, and allow the read/write,
list, and deletion operations needed for replication, recovery, and retention. Follow the provider
guides for exact permissions. Coordinate bucket lifecycle rules with Litestream retention so they
do not remove files needed for recovery.

## Application responsibilities

Use a local filesystem and keep the database and SQLite companion files on the shared volume.
The app and Litestream must run under the same Linux kernel; a macOS-native SQLite writer cannot
share locking safely with a Linux-container backup process. Configure WAL mode, a busy timeout,
and the application's durability policy in the app. Rootstock does not modify database pragmas,
schemas, migrations, transactions, or tenant authorization.

Backing up one SQLite database does not cover separate application files or make independently
replicated databases transactionally consistent. Define and test coordinated recovery for all state
your application needs. This template handles one database; extending the config to multiple
databases also requires explicit preparation and recovery for each.

## Verify locally

```sh
./tests/sqlite/run.sh
```

The drill uses the public template, disposable volumes, a synthetic Rust SQLite writer, and local
S3-compatible storage provided by a digest-pinned RustFS container. A one-off AWS CLI container
creates its test bucket. Both images support AMD64 and ARM64 and require no private registry
credentials. The writer is a separate Cargo workspace package (`sqlite-fixture`) with bundled
SQLite, built into a static binary in a scratch image. It adds no SQLite dependency to the rootstock
CLI and needs no Python runtime. Run its unit test with `cargo test -p sqlite-fixture` or include it
with `cargo test --workspace`.

The fixture reads `DATABASE_PATH` (default `/data/app.db`). Its default `serve` command holds a WAL
connection open; `health` checks the schema, `insert` writes the drill's marker, and `verify` checks
integrity and the recovered marker. `assert-missing` checks that recovery left no database;
`remove` deletes the disposable database and its SQLite sidecars to simulate data loss. Commands
that open SQLite require an existing file, so a failed recovery cannot silently create an empty one.

This object store is a disposable test dependency; deployments continue to use the
configured off-host S3-compatible provider. Built fixture images reuse `rootstock-sqlite-app:test`
and `rootstock-sqlite-backup:test`; reruns rebuild those tags and keep them cached. Test builds omit
provenance attestations so identical inputs retain the same image ID; changed contents still create
a new immutable image behind the existing tag. Only containers, volumes, and networks receive
run-specific names. The drill exercises the real rootstock deployment
command, missing-backup startup refusal, explicit initialization, live replication, no-overwrite
guards, restart safety, and recovery
into a fresh volume with a data marker and full SQLite integrity check. CI runs the same drill. Its memory
snapshot is diagnostic, not a production capacity claim. No existing applications or cloud buckets
are touched; real AWS/Spaces credentials and recovery still need their own deployment rehearsal.

Upstream references: [Docker deployment](https://litestream.io/guides/docker/),
[configuration](https://litestream.io/reference/config/), [restore](https://litestream.io/reference/restore/),
[S3](https://litestream.io/guides/s3/), and [Spaces](https://litestream.io/guides/digitalocean/).
