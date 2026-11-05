use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Resolves hosted console settings without starting containers or contacting a Docker engine.
fn dashboard(auth: Option<&str>) -> Output {
    let mut command = Command::new("docker");
    command
        .args(["compose", "-f"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/dashboard/compose.yml"))
        .args([
            "--project-name",
            "rootstock-dashboard",
            "config",
            "--format",
            "json",
        ])
        .env("APP_IMAGE", "rootstock-dashboard:test")
        .env("APP_DOMAIN", "dashboard.example.com")
        .env_remove("DASHBOARD_AUTH");
    if let Some(auth) = auth {
        command.env("DASHBOARD_AUTH", auth);
    }
    command.output().unwrap()
}

/// Prevents a public console deployment from omitting its authentication credentials.
#[test]
fn dashboard_refuses_missing_authentication() {
    let output = dashboard(None);
    assert!(
        !output.status.success(),
        "dashboard resolved without authentication"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("DASHBOARD_AUTH"));
}

/// Keeps the backend private and attaches authentication to its entire public router.
#[test]
fn dashboard_routes_require_authentication_without_publishing_a_backend_port() {
    let auth = "operator:$2y$12$not-a-real-hash";
    let output = dashboard(Some(auth));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let service = &config["services"]["dashboard"];
    let labels = &service["labels"];
    assert_eq!(
        labels["traefik.http.routers.rootstock-dashboard-console.middlewares"],
        "rootstock-dashboard-auth"
    );
    assert_eq!(
        labels["traefik.http.middlewares.rootstock-dashboard-auth.basicauth.users"]
            .as_str()
            .unwrap()
            .replace("$$", "$"),
        auth
    );
    assert_eq!(
        labels["traefik.http.middlewares.rootstock-dashboard-auth.basicauth.removeheader"],
        "true"
    );
    assert!(service["ports"].is_null());
    assert_eq!(
        service["environment"]["DOCKER_HOST"],
        "tcp://socket-proxy:2375"
    );
    assert_eq!(service["read_only"], true);
    assert!(
        service["volumes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|mount| mount["source"] != "/var/run/docker.sock")
    );
}

/// Rehearses the reusable deployment template with explicit release inputs.
fn deploy(image: &str, domain: &str) -> Output {
    let inventory = tempfile::tempdir().expect("the example inventory should be created");
    let app = inventory.path().join("web");
    fs::create_dir(&app).expect("the example application should be created");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/web/compose.yml"),
        app.join("compose.yml"),
    )
    .expect("the public template should be copied");
    Command::new(env!("CARGO_BIN_EXE_rootstock"))
        .arg("--apps")
        .arg(inventory.path())
        .args(["deploy", "web", "--dry-run"])
        .env("APP_IMAGE", image)
        .env("APP_DOMAIN", domain)
        .output()
        .expect("the CLI should run")
}

/// Ensures the deployment template works through the CLI without starting containers.
#[test]
fn rehearses_a_web_release() {
    let output = deploy("ghcr.io/example/web:release", "app.example.com");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("--project-name rootstock-web"), "{stdout}");
    assert!(stdout.contains("up -d --wait"), "{stdout}");
    assert!(!stdout.contains("\nran "), "{stdout}");
}

/// Refuses a release before touching Docker when the image or public domain is missing.
#[test]
fn refuses_incomplete_release_configuration() {
    for (image, domain, missing) in [
        ("", "app.example.com", "APP_IMAGE"),
        ("ghcr.io/example/web:release", "", "APP_DOMAIN"),
    ] {
        let output = deploy(image, domain);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(!output.status.success(), "release should have been refused");
        assert!(stderr.contains(missing), "{stderr}");
    }
}
