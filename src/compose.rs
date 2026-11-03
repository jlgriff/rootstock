use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::inventory::Discovered;

/// The arguments every Compose call for one application shares, scoping it to its own project.
fn base_args(app: &Discovered) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "--project-directory".to_string(),
        app.directory.display().to_string(),
        "--project-name".to_string(),
        app.project.clone(),
    ];
    for profile in &app.profiles {
        args.push("--profile".to_string());
        args.push(profile.clone());
    }
    args
}

/// The arguments that print one application's fully resolved configuration.
pub fn config_args(app: &Discovered) -> Vec<String> {
    let mut args = base_args(app);
    args.extend([
        "config".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ]);
    args
}

/// The arguments that bring one application up in the background.
pub fn up_args(app: &Discovered) -> Vec<String> {
    let mut args = base_args(app);
    args.extend([
        "up".to_string(),
        "-d".to_string(),
        "--wait".to_string(),
        "--remove-orphans".to_string(),
    ]);
    args
}

/// Resolves an application's Compose file, so its labels carry no unexpanded variables.
pub fn resolve(app: &Discovered) -> Result<Value> {
    let args = config_args(app);
    let output = Command::new("docker")
        .args(&args)
        .output()
        .with_context(|| format!("running `docker {}`", args.join(" ")))?;

    if !output.status.success() {
        bail!(
            "`docker {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("reading the resolved configuration of `{}`", app.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Builds an isolated application definition for the behavior under test.
    fn app(profiles: &[&str]) -> Discovered {
        Discovered {
            name: "web".to_string(),
            directory: PathBuf::from("/srv/rootstock/apps/web"),
            project: "rootstock-web".to_string(),
            profiles: profiles.iter().map(|p| p.to_string()).collect(),
        }
    }

    /// Makes command arguments readable in failure reports.
    fn joined(args: &[String]) -> String {
        args.join(" ")
    }

    #[test]
    /// Protects deployments from affecting another application.
    fn scopes_every_call_to_the_application_project_and_directory() {
        let args = joined(&config_args(&app(&[])));

        assert!(args.contains("--project-name rootstock-web"), "{args}");
        assert!(
            args.contains("--project-directory /srv/rootstock/apps/web"),
            "{args}"
        );
    }

    #[test]
    /// Keeps routing checks aware of services behind Compose profiles.
    fn resolving_activates_every_profile_or_profiled_services_stay_invisible() {
        let args = joined(&config_args(&app(&["prod", "backup"])));

        assert!(args.contains("--profile prod"), "{args}");
        assert!(args.contains("--profile backup"), "{args}");
    }

    #[test]
    /// Keeps inventory parsing independent of human-readable Compose output.
    fn resolving_asks_for_machine_readable_output() {
        let args = joined(&config_args(&app(&[])));

        assert!(args.contains("config"), "{args}");
        assert!(args.contains("--format json"), "{args}");
    }

    #[test]
    /// Keeps applications running independently after the CLI exits.
    fn bringing_up_runs_detached_and_clears_containers_left_behind() {
        let args = joined(&up_args(&app(&[])));

        assert!(args.contains("up -d"), "{args}");
        assert!(args.contains("--remove-orphans"), "{args}");
    }

    #[test]
    /// Prevents a release from reporting success before Compose readiness.
    fn bringing_up_waits_for_services_to_report_healthy() {
        let args = joined(&up_args(&app(&[])));

        assert!(args.contains("--wait"), "{args}");
    }

    #[test]
    /// Ensures deployment includes the services validated in inventory.
    fn bringing_up_activates_every_profile() {
        let args = joined(&up_args(&app(&["prod", "backup"])));

        assert!(args.contains("--profile prod"), "{args}");
        assert!(args.contains("--profile backup"), "{args}");
    }

    #[test]
    /// Protects application data from deployment commands.
    fn no_call_ever_destroys_a_volume() {
        for args in [config_args(&app(&["prod"])), up_args(&app(&["prod"]))] {
            let args = joined(&args);
            assert!(!args.contains(" down"), "{args}");
            assert!(!args.contains(" -v"), "{args}");
            assert!(!args.contains("--volumes"), "{args}");
        }
    }
}
