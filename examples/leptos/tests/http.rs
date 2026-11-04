use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

struct Server(Child);

impl Drop for Server {
    /// Reaps the disposable server even when a probe assertion fails.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Protects rendered content, embedded styling, readiness, and unknown-path behavior.
#[tokio::test]
async fn serves_a_self_contained_page_and_readiness() {
    let router = leptos_placeholder::router();
    let response = router
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let html = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("Under construction"), "{html}");
    assert!(html.contains("<h1"), "{html}");
    assert!(html.contains("@layer"), "compiled Tailwind CSS is missing");
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("<link"), "external assets are unnecessary");
    let response = router
        .clone()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 32).await.unwrap(), "ok\n");
    let response = router
        .oneshot(Request::get("/missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Exercises the real binary's healthcheck before and after its server exits.
#[test]
fn container_probe_requires_a_healthy_server() {
    let executable = env!("CARGO_BIN_EXE_leptos-placeholder");
    let mut server = Server(
        Command::new(executable)
            .env("SITE_ADDR", "127.0.0.1:0")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut url = String::new();
    BufReader::new(server.0.stdout.take().unwrap())
        .read_line(&mut url)
        .unwrap();
    let address = url.trim().strip_prefix("http://").expect("server URL");
    let probe = Command::new(executable)
        .env("SITE_ADDR", address)
        .arg("--healthcheck")
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    drop(server);
    let probe = Command::new(executable)
        .env("SITE_ADDR", address)
        .arg("--healthcheck")
        .output()
        .unwrap();
    assert!(
        !probe.status.success(),
        "a stopped server must fail readiness"
    );
}
