use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{compose, routes};

/// Prefixes every Compose project so one application's resources cannot collide with another's.
pub const PROJECT_PREFIX: &str = "rootstock-";

const COMPOSE_FILES: [&str; 2] = ["compose.yml", "docker-compose.yml"];

/// An application found on disk, before its Compose file has been resolved.
#[derive(Debug, PartialEq)]
pub struct Discovered {
    pub name: String,
    pub directory: PathBuf,
    pub project: String,
    /// Compose profiles to activate. Without these, profiled services stay invisible.
    pub profiles: Vec<String>,
}

/// An application whose Compose file has been resolved, so the domains it claims are known.
#[derive(Debug, PartialEq)]
pub struct App {
    pub discovered: Discovered,
    pub domains: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Settings {
    #[serde(default)]
    profiles: Vec<String>,
}

/// Finds every application below a directory: one folder holding one Compose file.
pub fn discover(root: &Path) -> Result<Vec<Discovered>> {
    let mut folders: Vec<_> = fs::read_dir(root)
        .with_context(|| format!("reading {}", root.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    folders.sort_by_key(|folder| folder.file_name());

    let mut found = Vec::new();
    for folder in folders {
        let directory = folder.path();
        if !COMPOSE_FILES
            .iter()
            .any(|file| directory.join(file).is_file())
        {
            continue;
        }

        let name = folder.file_name().to_string_lossy().to_string();
        found.push(Discovered {
            project: format!("{PROJECT_PREFIX}{name}"),
            profiles: read_settings(&directory)?.profiles,
            name,
            directory,
        });
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Finds every application and resolves what each one claims to serve.
pub fn load(root: &Path) -> Result<Vec<App>> {
    discover(root)?
        .into_iter()
        .map(|discovered| {
            let config = compose::resolve(&discovered)?;
            let domains = routes::domains_in_project(&config)
                .with_context(|| format!("application `{}`", discovered.name))?;
            Ok(App {
                discovered,
                domains,
            })
        })
        .collect()
}

/// Reads an application's rootstock settings, which only exist when it needs profiles.
fn read_settings(directory: &Path) -> Result<Settings> {
    let path = directory.join("rootstock.toml");
    if !path.is_file() {
        return Ok(Settings::default());
    }

    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an isolated application definition for the behavior under test.
    fn app(root: &Path, name: &str, files: &[(&str, &str)]) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        for (file, body) in files {
            fs::write(dir.join(file), body).unwrap();
        }
    }

    #[test]
    /// Keeps application registration driven by deployable files.
    fn finds_an_application_by_its_compose_file() {
        let root = tempfile::tempdir().unwrap();
        app(root.path(), "worker", &[("compose.yml", "services: {}\n")]);

        let found = discover(root.path()).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "worker");
        assert_eq!(found[0].directory, root.path().join("worker"));
    }

    #[test]
    /// Preserves discovery for existing Compose applications.
    fn accepts_the_older_compose_file_name() {
        let root = tempfile::tempdir().unwrap();
        app(
            root.path(),
            "legacy",
            &[("docker-compose.yml", "services: {}\n")],
        );

        assert_eq!(discover(root.path()).unwrap().len(), 1);
    }

    #[test]
    /// Protects unrelated applications from sharing deployment resources.
    fn scopes_each_application_to_its_own_compose_project() {
        let root = tempfile::tempdir().unwrap();
        app(root.path(), "worker", &[("compose.yml", "services: {}\n")]);

        assert_eq!(
            discover(root.path()).unwrap()[0].project,
            "rootstock-worker"
        );
    }

    #[test]
    /// Prevents profiled services from disappearing during discovery.
    fn reads_the_profiles_an_application_needs_activated() {
        let root = tempfile::tempdir().unwrap();
        app(
            root.path(),
            "web",
            &[
                ("compose.yml", "services: {}\n"),
                ("rootstock.toml", "profiles = [\"prod\", \"backup\"]\n"),
            ],
        );

        assert_eq!(
            discover(root.path()).unwrap()[0].profiles,
            ["prod", "backup"]
        );
    }

    #[test]
    /// Allows simple applications without extra settings.
    fn an_application_needing_no_profiles_declares_none() {
        let root = tempfile::tempdir().unwrap();
        app(root.path(), "worker", &[("compose.yml", "services: {}\n")]);

        assert!(discover(root.path()).unwrap()[0].profiles.is_empty());
    }

    #[test]
    /// Keeps non-application directories out of deployment inventory.
    fn ignores_a_folder_that_holds_no_compose_file() {
        let root = tempfile::tempdir().unwrap();
        app(
            root.path(),
            "notes",
            &[("README.md", "not an application\n")],
        );

        assert!(discover(root.path()).unwrap().is_empty());
    }

    #[test]
    /// Keeps inventory output and validation diagnostics deterministic.
    fn returns_applications_in_a_stable_order() {
        let root = tempfile::tempdir().unwrap();
        for name in ["web", "demo", "worker"] {
            app(root.path(), name, &[("compose.yml", "services: {}\n")]);
        }

        let names: Vec<String> = discover(root.path())
            .unwrap()
            .into_iter()
            .map(|found| found.name)
            .collect();
        assert_eq!(names, ["demo", "web", "worker"]);
    }

    #[test]
    /// Makes invalid application settings actionable.
    fn names_the_settings_file_it_could_not_read() {
        let root = tempfile::tempdir().unwrap();
        app(
            root.path(),
            "broken",
            &[
                ("compose.yml", "services: {}\n"),
                ("rootstock.toml", "profiles = "),
            ],
        );

        let error = format!("{:#}", discover(root.path()).unwrap_err());

        assert!(error.contains("broken/rootstock.toml"), "{error}");
    }
}
