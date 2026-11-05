use std::path::PathBuf;

use anyhow::{Result, ensure};
use clap::Parser;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{common::schema_for_output, router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use rootstock::{deployment, inventory, status, validate};
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(
    version,
    about = "Read-only MCP tools for the selected Rootstock inventory and Docker host"
)]
struct Cli {
    /// Local application inventory; use an absolute path in MCP client settings.
    #[arg(long, default_value = "apps")]
    apps: PathBuf,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StatusArgs {
    #[schemars(
        description = "Optional app name to select from the live snapshot; omit for all deployments."
    )]
    name: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct AppArgs {
    #[schemars(
        description = "Existing application name in the configured inventory, such as web."
    )]
    name: String,
}

#[derive(Serialize, schemars::JsonSchema)]
struct AppRegistration {
    name: String,
    project: String,
    profiles: Vec<String>,
    domains: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct AppList {
    apps: Vec<AppRegistration>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct RouteCheck {
    valid: bool,
    app_count: usize,
    conflicts: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct DeploymentPlan {
    #[serde(flatten)]
    plan: deployment::Plan,
    executed: bool,
    program: &'static str,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
enum Response<T> {
    Data(T),
    Error { error: String },
}

#[derive(Clone)]
struct RootstockServer {
    apps: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RootstockServer {
    /// Fixes inventory scope at process startup instead of accepting paths from tool calls.
    fn new(apps: PathBuf) -> Self {
        Self {
            apps,
            tool_router: Self::tool_router(),
        }
    }

    /// Keeps blocking Docker queries off the protocol loop and reports recoverable tool failures.
    async fn read<T: Serialize + Send + 'static>(
        &self,
        operation: impl FnOnce(PathBuf) -> Result<T> + Send + 'static,
    ) -> Result<CallToolResult, McpError> {
        let apps = self.apps.clone();
        let result = tokio::task::spawn_blocking(move || operation(apps))
            .await
            .map_err(|error| {
                McpError::internal_error(format!("Rootstock worker failed: {error}"), None)
            })?;
        let failed = result.is_err();
        let response = match result {
            Ok(value) => Response::Data(value),
            Err(error) => Response::Error {
                error: format!("{error:#}"),
            },
        };
        let value = serde_json::to_value(response)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(if failed {
            CallToolResult::structured_error(value)
        } else {
            CallToolResult::structured(value)
        })
    }

    /// Returns selected registration and routing fields without resolved Compose configuration.
    #[tool(
        description = "List registered Rootstock applications, profiles, and declared domains from the configured local inventory.",
        output_schema = schema_for_output::<Response<AppList>>(),
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_apps(
        &self,
        Parameters(_): Parameters<NoArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.read(|root| {
            let apps = inventory::load(&root)?
                .into_iter()
                .map(|app| AppRegistration {
                    name: app.discovered.name,
                    project: app.discovered.project,
                    profiles: app.discovered.profiles,
                    domains: app.domains,
                })
                .collect::<Vec<_>>();
            Ok(AppList { apps })
        })
        .await
    }

    /// Reuses the dashboard collector and optionally narrows its selected status fields to one app.
    #[tool(
        description = "Read live container state, domains, ports, uptime, restarts, memory, and CPU, plus host memory and swap where supported. Does not probe public reachability.",
        output_schema = schema_for_output::<Response<status::Snapshot>>(),
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_status(
        &self,
        Parameters(args): Parameters<StatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.read(move |root| {
            let mut snapshot = status::collect(&root)?;
            if let Some(name) = args.name {
                snapshot.apps.retain(|app| app.name == name);
                ensure!(
                    !snapshot.apps.is_empty(),
                    "no deployment named `{name}` on the selected host or in the local inventory"
                );
            }
            Ok(snapshot)
        })
        .await
    }

    /// Reports conflict details as a validation result instead of hiding them behind an error.
    #[tool(
        description = "Validate declared hostname ownership across the complete local inventory. Reports conflicts without changing applications or contacting their public endpoints.",
        output_schema = schema_for_output::<Response<RouteCheck>>(),
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn check_routes(
        &self,
        Parameters(_): Parameters<NoArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.read(|root| {
            let apps = inventory::load(&root)?;
            let conflicts = validate::conflicts(&apps);
            Ok(RouteCheck {
                valid: conflicts.is_empty(),
                app_count: apps.len(),
                conflicts,
            })
        })
        .await
    }

    /// Shares the CLI's deployment gate while returning an invocation that is never executed here.
    #[tool(
        description = "Validate the complete inventory and describe one existing app's Docker Compose deploy invocation. This is a configuration preflight, not a live infrastructure diff; no deployment is executed.",
        output_schema = schema_for_output::<Response<DeploymentPlan>>(),
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn plan_deploy(
        &self,
        Parameters(args): Parameters<AppArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.read(move |root| {
            let apps = inventory::load(&root)?;
            Ok(DeploymentPlan {
                plan: deployment::plan(&apps, &args.name)?,
                executed: false,
                program: "docker",
            })
        })
        .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RootstockServer {
    /// Advertises only read-only tools and describes the fixed local connection scope.
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).with_title("Rootstock"))
            .with_instructions("Read-only Rootstock inspection. The process configuration fixes the local inventory and Docker host/context. Inventory tools read local Compose definitions; get_status reads live Docker data. plan_deploy never executes its returned command. No deployment, restart, shell, file-writing, or credential tools are available.")
    }
}

/// Serves one local client session over stdio using a single-thread async runtime.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    RootstockServer::new(cli.apps)
        .serve(stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}
