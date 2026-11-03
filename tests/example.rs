use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Rehearses the public example release with explicit deployment inputs.
fn deploy(image: &str, domain: &str) -> Output {
    let inventory = tempfile::tempdir().expect("the example inventory should be created");
    let app = inventory.path().join("web");
    fs::create_dir(&app).expect("the example application should be created");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/web/compose.yml"),
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

/// Ensures the example application can be released through the CLI without starting containers.
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
