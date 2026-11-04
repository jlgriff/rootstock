#!/bin/sh
set -eu
unset COMPOSE_FILE COMPOSE_PROFILES
cd -- "$(dirname -- "$0")/../.."
work=$(mktemp -d)
name="sqlite-test-$$"
project="rootstock-$name"
app="$work/apps/$name"
mkdir -p "$work/apps"
cp -R templates/sqlite-litestream "$app"
export APP_IMAGE=rootstock-sqlite-app:test
export APP_DOMAIN=sqlite.example.test
export SQLITE_BACKUP_IMAGE=rootstock-sqlite-backup:test
export SQLITE_DATABASE=app.db SQLITE_UID=1000 SQLITE_GID=1000
export LITESTREAM_BUCKET=backups LITESTREAM_PREFIX=app.db
export LITESTREAM_REGION=us-east-1 LITESTREAM_ENDPOINT=http://object-store:9000
export AWS_ACCESS_KEY_ID=example-access AWS_SECRET_ACCESS_KEY=example-secret-key AWS_SESSION_TOKEN=
export SQLITE_DATA_VOLUME="$project-source"

# Targets only this run's temporary copy of the public template.
compose() {
  docker compose --project-directory "$app" -p "$project" "$@"
}

# Ensures failed recovery cannot leave a database that the app might treat as new.
assert_no_database() {
  docker run --rm --network none --mount "type=volume,source=$1,target=/data,readonly" \
    "$APP_IMAGE" assert-missing
}

# Removes only this drill's temporary containers, volumes, and network.
cleanup() {
  compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  docker volume rm "$project-source" "$project-recovered" "$project-missing" >/dev/null 2>&1 || true
  docker network rm "$project-proxy" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

cat > "$app/compose.override.yml" <<CONF
services:
  web:
    pull_policy: never
    labels:
      traefik.enable: "false"
  prepare:
    pull_policy: never
  backup:
    pull_policy: never
  object-store:
    image: rustfs/rustfs:1.0.4@sha256:ed9210d6411d77ffceb70d94b964a3cb7d225983368bc25bebe01e995293dcc2
    command: ["/data"]
    environment:
      RUSTFS_ACCESS_KEY: example-access
      RUSTFS_SECRET_KEY: example-secret-key
      RUSTFS_CONSOLE_ENABLE: "false"
      RUSTFS_OBS_LOGGER_LEVEL: error
    healthcheck:
      test: [CMD, curl, --fail, --silent, http://127.0.0.1:9000/health]
      interval: 1s
      timeout: 2s
      retries: 30
  object-init:
    image: public.ecr.aws/aws-cli/aws-cli@sha256:d557b3524475d70dd3104cf44d96568ee3188897b73a6ece2b37aa50e1af362f
    profiles: [test]
    environment:
      AWS_ACCESS_KEY_ID: example-access
      AWS_SECRET_ACCESS_KEY: example-secret-key
    command: ["--endpoint-url", "http://object-store:9000", "--region", "us-east-1", "s3api", "create-bucket", "--bucket", "backups"]
networks:
  rootstock-proxy:
    name: $project-proxy
CONF

docker build --provenance=false -q -t "$SQLITE_BACKUP_IMAGE" templates/sqlite-litestream >/dev/null
docker build --provenance=false -q -t "$APP_IMAGE" -f tests/sqlite/Dockerfile . >/dev/null
docker network create "$project-proxy" >/dev/null
compose up -d --wait object-store
compose run --rm --no-deps object-init >/dev/null

if cargo run -q -- --apps "$work/apps" deploy "$name" > "$work/start.log" 2>&1; then
  echo 'FAIL: application started without an initialized database or recoverable backup.' >&2
  exit 1
fi
assert_no_database "$project-source"
echo 'ok: empty volume with no backup blocks deployment'

compose run --rm --no-deps prepare initialize
cargo run -q -- --apps "$work/apps" deploy "$name"
compose exec -T web /sqlite-fixture insert
compose exec -T backup litestream sync -socket /tmp/litestream.sock -wait /data/app.db
docker stats --no-stream --format 'backup container memory: {{.MemUsage}}' "$(compose ps -q backup)"

if compose run --rm --no-deps prepare initialize > "$work/refuse.log" 2>&1; then
  echo 'FAIL: initialization accepted an existing database.' >&2
  exit 1
fi
if compose run --rm --no-deps prepare restore > "$work/refuse.log" 2>&1; then
  echo 'FAIL: restore accepted an existing database.' >&2
  exit 1
fi
echo 'ok: initialization and recovery refuse to replace existing data'

compose stop web backup
SQLITE_DATA_VOLUME="$project-recovered" cargo run -q -- --apps "$work/apps" deploy "$name"
compose exec -T web /sqlite-fixture verify
echo 'ok: fresh volume recovered the committed marker from object storage before app startup'

compose stop web backup
SQLITE_DATA_VOLUME="$project-recovered" compose run --rm --no-deps web remove
web=$(compose ps -aq web)
docker update --restart=no "$web" >/dev/null
docker start "$(compose ps -aq backup)" "$web" >/dev/null
attempt=0
while [ "$(docker inspect --format '{{.State.Running}}' "$web")" = true ]; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 10 ]; then
    compose logs --tail 15 backup web >&2
    echo 'FAIL: container restart accepted a missing database without preparation.' >&2
    exit 1
  fi
  sleep 1
done
[ "$(docker inspect --format '{{.State.ExitCode}}' "$web")" -ne 0 ]
compose stop web backup
assert_no_database "$project-recovered"
echo 'ok: container restart without preparation cannot silently initialize a database'

if SQLITE_DATA_VOLUME="$project-missing" LITESTREAM_PREFIX=missing.db cargo run -q -- --apps "$work/apps" deploy "$name" > "$work/start.log" 2>&1; then
  echo 'FAIL: missing backup allowed application startup.' >&2
  exit 1
fi
assert_no_database "$project-missing"
echo 'ok: recovery failure prevents application startup'
