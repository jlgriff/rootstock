use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::{compose, inventory::App, validate};

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "api-schema", derive(schemars::JsonSchema))]
pub struct Plan {
    pub name: String,
    pub project: String,
    pub domains: Vec<String>,
    pub docker_args: Vec<String>,
}

/// Validates the entire inventory before describing one application's deployment command.
pub fn plan(apps: &[App], name: &str) -> Result<Plan> {
    let conflicts = validate::conflicts(apps);
    if !conflicts.is_empty() {
        bail!(
            "{} routing conflicts, nothing deployed:\n{}",
            conflicts.len(),
            conflicts.join("\n")
        );
    }
    let app = apps
        .iter()
        .find(|app| app.discovered.name == name)
        .with_context(|| {
            format!("no application named `{name}` under the applications directory")
        })?;
    Ok(Plan {
        name: app.discovered.name.clone(),
        project: app.discovered.project.clone(),
        domains: app.domains.clone(),
        docker_args: compose::up_args(&app.discovered),
    })
}
