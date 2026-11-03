#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"

work=$(mktemp -d)
export ROOTSTOCK_ACCEPTANCE_RUN="acceptance-$$"
export ROOTSTOCK_PROXY_NETWORK="rootstock-$ROOTSTOCK_ACCEPTANCE_RUN"
apps="$work/apps"
mkdir -p "$apps"
for compose in apps/*/compose.yml; do
  fixture=$(dirname "$compose")
  cp -R "$fixture" "$apps/$ROOTSTOCK_ACCEPTANCE_RUN-$(basename "$fixture")"
done
failures=0

# Runs the CLI against this run's isolated application inventory.
rootstock() {
  cargo run -q --manifest-path ../../Cargo.toml -- --apps "$apps" "$@"
}

# Starts the production proxy with local HTTP and run-scoped discovery.
proxy() {
  ACME_EMAIL=acceptance@example.invalid docker compose \
    -f ../../proxy/compose.yml -f ../../proxy/compose.test.yml \
    -p "rootstock-$ROOTSTOCK_ACCEPTANCE_RUN-proxy" "$@"
}

# Removes only the Compose projects and network created by this run.
cleanup() {
  for app in "$apps"/*/; do
    docker compose --project-directory "$app" \
      --project-name "rootstock-$(basename "$app")" --profile prod \
      down --remove-orphans >/dev/null 2>&1 || true
  done
  proxy down --remove-orphans --volumes >/dev/null 2>&1 || true
  docker network rm "$ROOTSTOCK_PROXY_NETWORK" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

docker network create "$ROOTSTOCK_PROXY_NETWORK" >/dev/null
proxy up -d --quiet-pull >/dev/null
base="http://$(proxy port traefik 80)"

echo "Discovered:"
rootstock list | sed 's/^/  /'
domains=$(rootstock list | grep -c '^  ' || true)
if [ "$domains" -eq 5 ]; then
  echo "  ok    found all 5 declared domains"
else
  echo "  FAIL  expected 5 domains, found $domains"
  failures=$((failures + 1))
fi
rootstock check >/dev/null

for app in "$apps"/*/; do
  rootstock deploy "$(basename "$app")" >/dev/null
done

for host in demo.test www.demo.test api.demo.test admin.demo.test web.test; do
  attempt=0
  until curl -sf -o /dev/null -H "Host: $host" "$base/"; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 40 ]; then
      echo "Traefik never routed $host. Logs:" >&2
      proxy logs --tail 40 traefik >&2
      exit 1
    fi
    sleep 1
  done
done

# Reads the application selected by a hostname.
body() { curl -fsS -H "Host: $1" "$base/"; }

# Reports the response status without treating a rejection as a curl failure.
code() { curl -sS -o /dev/null -w '%{http_code}' -H "Host: $1" "$base/"; }

# Checks that a domain serves only its assigned content.
serves() {
  got=$(body "$1")
  if [ "$got" = "$2" ]; then
    echo "  ok    $1 -> $2"
  else
    echo "  FAIL  $1 -> expected '$2', got '$got'"
    failures=$((failures + 1))
  fi
}

serves demo.test WEBSITE
serves www.demo.test WEBSITE
serves api.demo.test API
serves admin.demo.test API
serves web.test WEB

unknown_code=$(code unknown.test)
if [ "$unknown_code" = 404 ]; then
  echo "  ok    unknown.test rejected (HTTP 404)"
else
  echo "  FAIL  unknown.test returned HTTP $unknown_code"
  failures=$((failures + 1))
fi

mkdir -p "$apps/$ROOTSTOCK_ACCEPTANCE_RUN-rival"
cat > "$apps/$ROOTSTOCK_ACCEPTANCE_RUN-rival/compose.yml" <<'CONF'
services:
  rival:
    image: nginx:alpine
    labels:
      traefik.enable: "true"
      traefik.http.routers.acceptance-rival.rule: "Host(`demo.test`)"
CONF

if rootstock check >/dev/null 2>&1; then
  echo "  FAIL  check accepted two applications claiming demo.test"
  failures=$((failures + 1))
else
  echo "  ok    check refused a second claim on demo.test"
fi

if rootstock deploy "$ROOTSTOCK_ACCEPTANCE_RUN-rival" >/dev/null 2>&1; then
  echo "  FAIL  deploy went ahead despite the conflict"
  failures=$((failures + 1))
else
  echo "  ok    deploy refused while the conflict stands"
fi

if [ "$failures" -gt 0 ]; then
  echo "$failures assertion(s) failed."
  exit 1
fi
echo "All routing assertions passed."
