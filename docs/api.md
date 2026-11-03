# API reference

Rootstock exposes local MCP tools and a local HTML console. Application APIs hosted through
Traefik belong to their applications and are not Rootstock endpoints.

| Interface | Operations | Access |
| --- | --- | --- |
| MCP stdio | `list_apps`, `get_status`, `check_routes`, `plan_deploy` | Client-launched `rootstock-mcp` process |
| Console HTTP | `GET /`, optionally with `inspect` | Loopback listener started by `rootstock observe` |

Use [MCP setup](mcp.md) for client configuration and [console configuration](configuration.md#live-status-and-local-dashboard)
for port and refresh settings. The [OpenAPI document](http.openapi.yaml) describes the HTTP route.
MCP's machine-readable tool contracts are returned by `tools/list`; they are generated from Rust types.

## MCP transport and results

The server uses the official SDK's stdio transport and protocol negotiation. No HTTP URL, API key,
or OAuth service is involved. A client initializes the session before invoking tools. Tool names
and application-name arguments are case-sensitive. Argument objects reject unknown fields.
All tools advertise `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`, and
`openWorldHint: true`; reads may contact the selected Docker host.

The examples below show the initialized-session JSON-RPC shape. The SDK may add metadata or
completion fields for the negotiated protocol version. See the [MCP tool specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
for protocol-level discovery and invocation.

Successful results contain both the returned object in `structuredContent` and its JSON encoding
in a `content` text block, with `isError: false`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [{"type": "text", "text": "{\"valid\":true,\"app_count\":1,\"conflicts\":[]}"}],
    "structuredContent": {"valid": true, "app_count": 1, "conflicts": []},
    "isError": false
  }
}
```

The per-tool response examples below show only `structuredContent`. Every output schema accepts
that tool's success object or an application error object with an `error` string.

### Error contract

| Condition | Result |
| --- | --- |
| Docker or Compose failure, unreadable inventory, invalid routing syntax, unknown app, conflicting deployment plan | Tool result with `isError: true`, `structuredContent: {"error": "..."}`, and a matching JSON text block |
| Wrong tool-argument field types, missing required tool fields, or unknown tool-argument fields | SDK tool result with `isError: true` and diagnostic text; structured content may be absent |
| Unknown tool name, including `deploy` or `restart` | JSON-RPC error `-32602`, `tool not found` |
| Malformed protocol parameters, such as a non-object `arguments` value | SDK JSON-RPC invalid-parameters error `-32602` |
| Internal worker or serialization failure | JSON-RPC error `-32603` |

Errors do not intentionally close the session. Fix the condition and retry. Error messages are
diagnostics, not stable identifiers; branch on `isError` or the JSON-RPC error code. Each call reads
current inputs; no previous successful snapshot is returned after a failed collection.

Example application failure:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [{"type": "text", "text": "{\"error\":\"docker ps failed: Docker host unavailable\"}"}],
    "structuredContent": {"error": "docker ps failed: Docker host unavailable"},
    "isError": true
  }
}
```

### `list_apps`

Resolves the configured local inventory and returns selected registration fields. It does not
discover unregistered live projects; use `get_status` for those. A missing inventory directory,
invalid Compose configuration, or unsupported routing declaration returns an application error.
Duplicate hostname ownership is reported by `check_routes`, not by listing.

Arguments: an empty object. No fields are accepted.

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_apps","arguments":{}}}
```

Response:

```json
{
  "apps": [
    {"name": "web", "project": "rootstock-web", "profiles": ["prod"], "domains": ["app.example.com"]}
  ]
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `apps` | array | Registered apps in name order; empty for an existing empty inventory |
| `apps[].name` | string | App directory name |
| `apps[].project` | string | Full Compose project identifier |
| `apps[].profiles` | string array | Profiles used during resolution and deployment |
| `apps[].domains` | string array | Explicit hostnames declared in resolved Traefik routing |

Resolved Compose configuration, container environment values, and image credentials are excluded.

### `get_status`

Reads live Rootstock containers from the selected Docker engine and joins optional local
registration. A missing inventory directory is allowed. Registered apps with no containers have
an empty `services` array. Live `rootstock-` projects absent from local registration are included.

| Argument | Type | Required? | Meaning |
| --- | --- | --- | --- |
| `name` | string or `null` | No | Exact app name; omit or use `null` for every deployment |

```json
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_status","arguments":{"name":"web"}}}
```

Response:

```json
{
  "captured_at": "2026-01-01T12:00:00Z",
  "apps": [{
    "name": "web",
    "project": "rootstock-web",
    "configured": true,
    "services": [{
      "service": "web",
      "container": "rootstock-web-web-1",
      "state": "running",
      "exit_code": null,
      "restart_policy": "unless-stopped",
      "health": "healthy",
      "uptime_seconds": 120,
      "restart_count": 0,
      "domains": ["app.example.com"],
      "routing_error": null,
      "ports": [{"internal": "3000/tcp", "published": []}],
      "memory": "4MiB / 1GiB",
      "cpu": "0.25%"
    }]
  }]
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `captured_at` | string | UTC RFC 3339 timestamp after collection; individual Docker reads are not atomic |
| `apps` | array | Deployments ordered by project; empty when none are found without a name filter |
| `apps[].name`, `project` | string | App name and full Compose project identifier |
| `apps[].configured` | boolean | Whether the project has a local inventory folder |
| `apps[].services` | array | Containers ordered by service and container name |
| `services[].service`, `container` | string | Compose service and Docker container name |
| `services[].state` | string | Raw Docker state, such as `running`, `exited`, or `restarting` |
| `services[].exit_code` | integer or `null` | Reported exit code for a nonrunning container |
| `services[].restart_policy` | string or `null` | Docker policy, such as `no`, `always`, or `unless-stopped` |
| `services[].health` | string or `null` | Docker health status; `null` when no check is recorded |
| `services[].uptime_seconds` | nonnegative integer or `null` | Current running-container uptime; `null` when not running |
| `services[].restart_count` | nonnegative integer | Restart count for the current container |
| `services[].domains` | string array | Hostnames from live Traefik labels |
| `services[].routing_error` | string or `null` | Per-container routing parse error; the rest of the snapshot remains available |
| `services[].ports` | array | Ports declared by Docker or Traefik metadata |
| `ports[].internal` | string | Container port/protocol, such as `3000/tcp` |
| `ports[].published` | string array | Host bindings, such as `127.0.0.1:8080` or `[::]:8080`; empty for unpublished ports |
| `services[].memory` | string or `null` | Docker-formatted memory usage and limit; `null` when unavailable |
| `services[].cpu` | string or `null` | Docker-formatted CPU percentage; `null` when unavailable |

`services[]` is nested under each `apps[]`; `ports[]` is nested under each service. Metrics are
formatted strings, not byte counts. Stopped containers have unavailable resource samples. Limits
are not reserved memory; container memory does not account for the whole VM. Domains and port
metadata do not establish public reachability or prove that a process is listening.

Unknown names return an application error. The name filter is applied after collecting the host
snapshot, so it does not reduce the underlying Docker query scope. Invalid live routing produces
`routing_error`; Docker failure or unreadable local registration fails the call.

### `check_routes`

Resolves every local app and checks hostname ownership. This validates local declarations, not
the live proxy, DNS, certificates, or running services. An empty inventory has no conflicts and
does not establish that an app is deployed.

Arguments: an empty object. No fields are accepted.

```json
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"check_routes","arguments":{}}}
```

Response with a conflict:

```json
{
  "valid": false,
  "app_count": 2,
  "conflicts": ["domain `app.example.com` is claimed by both `web` and `worker`"]
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `valid` | boolean | True when no hostname/name conflicts are found |
| `app_count` | nonnegative integer | Number of registered apps resolved |
| `conflicts` | string array | Conflict diagnostics; empty when valid |

Conflicts are a successful validation response with `isError: false` and `valid: false`.
Malformed routing rules, failed Compose resolution, and unreadable inventory return application
errors instead of a partial validation result.

### `plan_deploy`

Uses the same deployment planner as `rootstock deploy`. The entire local inventory must resolve
and pass conflict checks before the requested app is selected. The returned invocation is never
executed by the MCP server.

| Argument | Type | Required? | Meaning |
| --- | --- | --- | --- |
| `name` | string | Yes | Exact registered application name; filesystem paths are not accepted as targets |

```json
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"plan_deploy","arguments":{"name":"web"}}}
```

Response:

```json
{
  "name": "web",
  "project": "rootstock-web",
  "domains": ["app.example.com"],
  "program": "docker",
  "docker_args": [
    "compose", "--project-directory", "/srv/rootstock/apps/web",
    "--project-name", "rootstock-web", "--profile", "prod",
    "up", "-d", "--wait", "--remove-orphans"
  ],
  "executed": false
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `name`, `project` | string | Selected app name and Compose project |
| `domains` | string array | Its resolved hostname declarations |
| `program` | string | Always `docker` |
| `docker_args` | string array | Ordered argument vector; preserve argument boundaries if reviewing it elsewhere |
| `executed` | boolean | Always false |

Missing apps, inventory/Compose errors, invalid routing, or conflicts anywhere in the inventory
return application errors. This is configuration preflight, not a live deployment diff or a
guarantee of image availability, backups, database readiness, or successful startup. Actual CLI
deployment can recreate containers and remove orphans; this tool only describes that invocation.

## Console HTTP

### `GET /`

Returns the current server-rendered HTML dashboard. The listener defaults to
`http://127.0.0.1:8787`; `observe --port` changes the local port. It is a separate interface from
MCP, with no JSON status route or mutation endpoints.

| Parameter | Location | Required? | Meaning |
| --- | --- | --- | --- |
| `inspect` | query | No | Zero-based unsigned decimal deployment index in the current project-sorted snapshot; opens its container details |
| `Host` | header | Yes | Exactly `127.0.0.1:<port>` or `localhost:<port>` |

```sh
curl http://127.0.0.1:8787/
curl 'http://127.0.0.1:8787/?inspect=0'
```

Only `/` and `/?inspect=<unsigned-index>` are accepted. Other query parameters, duplicate
parameters, and invalid index syntax return 404. An out-of-range numeric index renders the
overview with all details closed. Indices describe snapshot order, not persistent identifiers.
Each request collects fresh data. Automatic refresh retains the current URL and selected index.

| Status | Body | Meaning |
| --- | --- | --- |
| `200` | HTML overview | Collection succeeded, including an empty inventory/host |
| `400` | `Unexpected Host header` | Host header is outside the accepted loopback names |
| `404` | `Not found` | Unsupported path or query syntax |
| `405` | `Only GET is supported` | Method is not GET, including HEAD |
| `503` | HTML error view | Docker/SSH, collection, or local registration failed; connection diagnostics are expandable |

The Host check runs before method and path checks. Responses set `Content-Type: text/html;
charset=utf-8`, `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, and the console's
Content Security Policy. No CORS access is advertised. Assets are embedded; no external asset
requests or browser JavaScript are needed. HTTP authentication is not implemented; access is
restricted by the loopback listener and Host check. Reads use the operator's configured Docker/SSH
credentials. The console adds no public proxy route.
