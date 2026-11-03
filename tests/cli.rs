//! These drive the real binary, which resolves Compose files, so they need Docker.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Runs the CLI against a throwaway directory of applications.
fn run(apps: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rootstock"))
        .arg("--apps")
        .arg(apps)
        .args(args)
        .output()
        .expect("the CLI should run")
}

/// Writes one application: a Compose file, and settings when it needs profiles.
fn app(root: &Path, name: &str, compose: &str, profiles: Option<&str>) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("compose.yml"), compose).unwrap();
    if let Some(profiles) = profiles {
        fs::write(dir.join("rootstock.toml"), profiles).unwrap();
    }
}

/// A Compose file whose one service claims the given host through Traefik.
fn claiming(host: &str) -> String {
    format!(
        r#"services:
  web:
    image: nginx:alpine
    labels:
      traefik.enable: "true"
      traefik.http.routers.web.rule: "Host(`{host}`)"
"#
    )
}

#[test]
/// Exposes the resolved application inventory through the real CLI.
fn list_shows_the_domains_a_compose_file_claims() {
    let dir = tempfile::tempdir().unwrap();
    app(dir.path(), "web", &claiming("app.example.com"), None);

    let output = run(dir.path(), &["list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("web (rootstock-web)"), "{stdout}");
    assert!(stdout.contains("app.example.com"), "{stdout}");
}

#[test]
/// Allows a non-conflicting inventory through the CLI.
fn check_accepts_applications_claiming_different_domains() {
    let dir = tempfile::tempdir().unwrap();
    app(dir.path(), "web", &claiming("app.example.com"), None);
    app(dir.path(), "worker", &claiming("worker.example.com"), None);

    assert!(run(dir.path(), &["check"]).status.success());
}

#[test]
/// Makes duplicate-domain failures actionable before deployment.
fn check_fails_and_names_the_domain_two_applications_claim() {
    let dir = tempfile::tempdir().unwrap();
    app(dir.path(), "web", &claiming("app.example.com"), None);
    app(dir.path(), "other", &claiming("app.example.com"), None);

    let output = run(dir.path(), &["check"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "check should have failed");
    assert!(stderr.contains("app.example.com"), "{stderr}");
}

#[test]
/// Allows release commands to be reviewed without starting containers.
fn deploy_rehearses_the_compose_call_without_running_it() {
    let dir = tempfile::tempdir().unwrap();
    app(dir.path(), "web", &claiming("app.example.com"), None);

    let output = run(dir.path(), &["deploy", "web", "--dry-run"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("would run"), "{stdout}");
    assert!(stdout.contains("--project-name rootstock-web"), "{stdout}");
    assert!(stdout.contains("up -d"), "{stdout}");
    assert!(!stdout.contains("\nran "), "{stdout}");
}

#[test]
/// Keeps the CLI release consistent with the validated inventory.
fn deploy_activates_the_profiles_an_application_declares() {
    let dir = tempfile::tempdir().unwrap();
    app(
        dir.path(),
        "web",
        &claiming("app.example.com"),
        Some("profiles = [\"prod\", \"backup\"]\n"),
    );

    let output = run(dir.path(), &["deploy", "web", "--dry-run"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("--profile prod"), "{stdout}");
    assert!(stdout.contains("--profile backup"), "{stdout}");
}

#[test]
/// Refuses an unknown release target before running Compose.
fn deploy_names_an_application_that_has_no_compose_file() {
    let dir = tempfile::tempdir().unwrap();
    app(dir.path(), "web", &claiming("app.example.com"), None);

    let output = run(dir.path(), &["deploy", "not-a-real-app", "--dry-run"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "deploy should have failed");
    assert!(stderr.contains("not-a-real-app"), "{stderr}");
}

#[test]
/// Protects existing routes from conflicting releases.
fn deploy_refuses_while_two_applications_claim_one_domain() {
    let dir = tempfile::tempdir().unwrap();
    app(dir.path(), "web", &claiming("app.example.com"), None);
    app(dir.path(), "other", &claiming("app.example.com"), None);

    let output = run(dir.path(), &["deploy", "web", "--dry-run"]);

    assert!(!output.status.success(), "deploy should have refused");
}

#[test]
/// Blocks catch-all rules before a release can reach the Docker engine.
fn deploy_refuses_an_unbounded_route() {
    let dir = tempfile::tempdir().unwrap();
    let compose = claiming("app.example.com").replace(
        "Host(`app.example.com`)",
        "Host(`app.example.com`) || PathPrefix(`/`)",
    );
    app(dir.path(), "web", &compose, None);

    let output = run(dir.path(), &["deploy", "web", "--dry-run"]);

    assert!(
        !output.status.success(),
        "unbounded route passed deployment checks"
    );
}

#[test]
/// Identifies an incomplete public service before deployment.
fn a_service_that_opts_in_without_a_router_is_reported_by_name() {
    let dir = tempfile::tempdir().unwrap();
    app(
        dir.path(),
        "broken",
        "services:\n  web:\n    image: nginx:alpine\n    labels:\n      traefik.enable: \"true\"\n",
        None,
    );

    let output = run(dir.path(), &["check"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "check should have failed");
    assert!(stderr.contains("web"), "{stderr}");
}
