#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;

use rmcp::{
    RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService,
    transport::TokioChildProcess,
};
use serde_json::{Value, json};

struct Fixture {
    directory: tempfile::TempDir,
}

impl Fixture {
    /// Builds a private inventory and records all Docker calls against synthetic data.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        for name in ["web", "worker"] {
            fs::create_dir_all(directory.path().join("apps").join(name)).unwrap();
            fs::write(
                directory.path().join("apps").join(name).join("compose.yml"),
                "services: {}\n",
            )
            .unwrap();
        }
        fs::write(
            directory.path().join("apps/web/rootstock.toml"),
            "profiles = [\"prod\"]\n",
        )
        .unwrap();
        fs::write(directory.path().join("docker"), r#"#!/bin/sh
printf '%s\n' "$*" >> "$MCP_TEST_DIRECTORY/calls"
if [ -f "$MCP_TEST_DIRECTORY/fail" ]; then
  echo 'synthetic Docker host unavailable' >&2
  exit 1
fi
case "$1" in
  compose)
    case "$*" in
      *rootstock-worker*) domain=worker.example.test ;;
      *) domain=app.example.test ;;
    esac
    if [ -f "$MCP_TEST_DIRECTORY/conflict" ]; then domain=app.example.test; fi
    cat <<JSON
{"services":{"web":{"environment":{"SECRET":"must-never-appear"},"labels":{"traefik.enable":"true","traefik.http.routers.web.rule":"Host(\u0060$domain\u0060)","traefik.http.services.web.loadbalancer.server.port":"3000"}}}}
JSON
    ;;
  ps) printf 'web-id rootstock-web\n' ;;
  inspect) printf '%s\n' '[{"Id":"web-id","Name":"/example-web","RestartCount":0,"Config":{"Env":["SECRET=must-never-appear"],"Labels":{"com.docker.compose.project":"rootstock-web","com.docker.compose.service":"web","traefik.enable":"true","traefik.http.routers.web.rule":"Host(\u0060app.example.test\u0060)","traefik.http.services.web.loadbalancer.server.port":"3000"}},"State":{"Status":"running","Running":true,"StartedAt":"2020-01-01T00:00:00Z","Health":{"Status":"healthy"}},"NetworkSettings":{"Ports":{"3000/tcp":null}}}]' ;;
  stats) printf '%s\n' '{"ID":"web-id","MemUsage":"4MiB / 1GiB","CPUPerc":"0.25%"}' ;;
  *) echo "unexpected Docker operation: $*" >&2; exit 1 ;;
esac
"#).unwrap();
        fs::set_permissions(
            directory.path().join("docker"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        Self { directory }
    }

    /// Builds a client-scoped command without changing the test process's environment.
    fn command(&self) -> tokio::process::Command {
        let path = std::env::join_paths(
            std::iter::once(self.directory.path().to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_rootstock-mcp"));
        command
            .arg("--apps")
            .arg(self.directory.path().join("apps"))
            .env("PATH", path)
            .env("MCP_TEST_DIRECTORY", self.directory.path())
            .stderr(Stdio::inherit());
        command
    }

    /// Connects a real MCP client to the built binary using its stdio transport.
    async fn connect(&self) -> RunningService<RoleClient, ()> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            ().serve(TokioChildProcess::new(self.command()).unwrap()),
        )
        .await
        .unwrap()
        .unwrap()
    }

    /// Reads the attempted Docker operations so tests can detect accidental mutations.
    fn calls(&self) -> String {
        fs::read_to_string(self.directory.path().join("calls")).unwrap_or_default()
    }
}

/// Keeps the documented initialized-session API usable by clients on the earlier protocol.
#[tokio::test]
async fn legacy_stdio_clients_can_discover_and_call_the_documented_tools() {
    let fixture = Fixture::new();
    let client = rmcp::model::ClientConfig::default()
        .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_11_25)
        .serve(TokioChildProcess::new(fixture.command()).unwrap())
        .await
        .unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 4);
    let response = client
        .call_tool(CallToolRequestParams::new("list_apps").with_arguments(rmcp::object!({})))
        .await
        .unwrap();
    assert_eq!(
        response.structured_content.unwrap()["apps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    client.cancel().await.unwrap();
}

/// Calls a named tool with structured arguments through the MCP session.
async fn call(
    client: &RunningService<RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> rmcp::model::CallToolResult {
    client
        .call_tool(
            CallToolRequestParams::new(name.to_string())
                .with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap()
}

/// Protects the public tool contract and prevents discovery from contacting Docker.
#[tokio::test]
async fn discovery_advertises_only_read_only_tools_without_querying_docker() {
    let fixture = Fixture::new();
    let client = fixture.connect().await;
    assert_eq!(
        client
            .peer_info()
            .unwrap()
            .server_info
            .as_ref()
            .unwrap()
            .name,
        "rootstock-mcp"
    );
    let mut tools = client.list_all_tools().await.unwrap();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>(),
        ["check_routes", "get_status", "list_apps", "plan_deploy"]
    );
    for tool in tools {
        assert!(
            tool.output_schema.is_some(),
            "missing response schema for {}",
            tool.name
        );
        let annotations = tool.annotations.unwrap();
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
    }
    assert!(fixture.calls().is_empty());
    client.cancel().await.unwrap();
}

/// Keeps shared inventory, status, and deployment planning structured and free of environment secrets.
#[tokio::test]
async fn tools_return_selected_data_and_plan_without_executing() {
    let fixture = Fixture::new();
    let client = fixture.connect().await;
    let apps = call(&client, "list_apps", json!({})).await;
    assert_ne!(apps.is_error, Some(true), "{apps:?}");
    let apps = apps.structured_content.unwrap();
    assert_eq!(apps["apps"].as_array().unwrap().len(), 2);
    assert_eq!(apps["apps"][0]["name"], "web");
    assert_eq!(apps["apps"][0]["domains"][0], "app.example.test");
    let routes = call(&client, "check_routes", json!({})).await;
    assert_eq!(routes.structured_content.unwrap()["valid"], true);
    let status = call(&client, "get_status", json!({"name":"web"})).await;
    let status = status.structured_content.unwrap();
    assert_eq!(status["apps"].as_array().unwrap().len(), 1);
    assert_eq!(
        status["apps"][0]["services"][0]["ports"][0]["internal"],
        "3000/tcp"
    );
    assert_eq!(status["apps"][0]["services"][0]["memory"], "4MiB / 1GiB");
    let plan = call(&client, "plan_deploy", json!({"name":"web"})).await;
    assert_ne!(plan.is_error, Some(true), "{plan:?}");
    let plan = plan.structured_content.unwrap();
    assert_eq!(plan["project"], "rootstock-web");
    assert_eq!(plan["executed"], false);
    let args = plan["docker_args"].as_array().unwrap();
    assert!(
        args.windows(2)
            .any(|pair| pair == [json!("--profile"), json!("prod")])
    );
    assert!(
        args.windows(3)
            .any(|part| part == [json!("up"), json!("-d"), json!("--wait")])
    );
    assert!(!format!("{apps}{status}{plan}").contains("must-never-appear"));
    assert!(!fixture.calls().lines().any(|line| {
        line.split_whitespace()
            .any(|arg| matches!(arg, "up" | "down" | "restart" | "rm"))
    }));
    client.cancel().await.unwrap();
}

/// Refuses a deployment plan when any app in the inventory claims a conflicting domain.
#[tokio::test]
async fn route_conflicts_are_reported_and_block_planning() {
    let fixture = Fixture::new();
    fs::write(fixture.directory.path().join("conflict"), "").unwrap();
    let client = fixture.connect().await;
    let routes = call(&client, "check_routes", json!({})).await;
    assert_ne!(routes.is_error, Some(true));
    let routes = routes.structured_content.unwrap();
    assert_eq!(routes["valid"], false);
    assert!(
        routes["conflicts"][0]
            .as_str()
            .unwrap()
            .contains("app.example.test")
    );
    let plan = call(&client, "plan_deploy", json!({"name":"web"})).await;
    assert_eq!(plan.is_error, Some(true));
    assert!(
        serde_json::to_string(&plan)
            .unwrap()
            .contains("routing conflicts")
    );
    assert!(!fixture.calls().contains(" up "));
    client.cancel().await.unwrap();
}

/// Surfaces Docker failure and invalid tool arguments without ending the client session.
#[tokio::test]
async fn failures_and_attempted_scope_overrides_are_refused() {
    let fixture = Fixture::new();
    let client = fixture.connect().await;
    fs::write(fixture.directory.path().join("fail"), "").unwrap();
    let result = call(&client, "get_status", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    assert!(
        serde_json::to_string(&result)
            .unwrap()
            .contains("synthetic Docker host unavailable")
    );
    fs::remove_file(fixture.directory.path().join("fail")).unwrap();
    let result = client
        .call_tool(
            CallToolRequestParams::new("get_status")
                .with_arguments(rmcp::object!({"apps":"/outside-inventory"})),
        )
        .await;
    assert!(result.is_err() || result.unwrap().is_error == Some(true));
    let result = call(&client, "plan_deploy", json!({"name":"../outside"})).await;
    assert_eq!(result.is_error, Some(true));
    let result = client
        .call_tool(
            CallToolRequestParams::new("deploy").with_arguments(rmcp::object!({"name":"web"})),
        )
        .await;
    assert!(result.is_err() || result.unwrap().is_error == Some(true));
    assert_ne!(
        call(&client, "list_apps", json!({})).await.is_error,
        Some(true)
    );
    client.cancel().await.unwrap();
}
