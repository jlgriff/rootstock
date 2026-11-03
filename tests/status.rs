#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

struct Fixture {
    directory: tempfile::TempDir,
}

impl Fixture {
    /// Supplies deterministic Docker responses without a daemon or real application data.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("apps/web")).unwrap();
        fs::write(
            directory.path().join("apps/web/compose.yml"),
            "unresolved: ${MISSING}",
        )
        .unwrap();
        fs::write(directory.path().join("inspect.json"), r#"[
          {"Id":"web-id","Name":"/example-web","RestartCount":2,
           "Config":{"Env":["SECRET=must-never-appear"],"ExposedPorts":null,"Labels":{
             "com.docker.compose.project":"rootstock-web","com.docker.compose.service":"web",
             "traefik.enable":"true","traefik.http.routers.web.rule":"Host(`app.example.com`)",
             "traefik.http.services.web.loadbalancer.server.port":"3000"}},
           "State":{"Status":"running","Running":true,"StartedAt":"2026-01-01T00:00:00Z","Health":{"Status":"healthy"}},
           "NetworkSettings":{"Ports":{"3000/tcp":[{"HostIp":"127.0.0.1","HostPort":"8080"}]}}},
          {"Id":"worker-id","Name":"/example-worker","RestartCount":0,
           "Config":{"Labels":{"com.docker.compose.project":"rootstock-web","com.docker.compose.service":"worker<script>"}},
           "State":{"Status":"exited","Running":false,"StartedAt":"2026-01-01T00:00:00Z"},
           "NetworkSettings":{"Ports":null}}
        ]"#).unwrap();
        fs::write(
            directory.path().join("ps"),
            "web-id rootstock-web\nworker-id rootstock-web\nother-id unrelated\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("docker"),
            r#"#!/bin/sh
case "$1" in
  ps) cat "$FIXTURE_DIRECTORY/ps" ;;
  context) printf '"%s"\n' "${FIXTURE_DOCKER_ENDPOINT:-unix:///example/docker.sock}" ;;
  inspect) cat "$FIXTURE_DIRECTORY/inspect.json" ;;
  stats) printf '%s\n' '{"ID":"web-id","MemUsage":"4MiB / 1GiB","CPUPerc":"0.25%"}' ;;
  *) echo "unexpected Docker command: $*" >&2; exit 1 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(
            directory.path().join("docker"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        Self { directory }
    }

    /// Launches the real CLI with a private Docker executable and inventory.
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rootstock"));
        let path = std::env::join_paths(
            std::iter::once(self.directory.path().to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        command
            .env("PATH", path)
            .env_remove("DOCKER_HOST")
            .env_remove("DOCKER_CONTEXT")
            .env("FIXTURE_DIRECTORY", self.directory.path())
            .arg("--apps")
            .arg(self.directory.path().join("apps"));
        command
    }

    /// Keeps synthetic discovery aligned with the inspected container set.
    fn containers(&self, containers: &Value) {
        fs::write(
            self.directory.path().join("inspect.json"),
            serde_json::to_vec(containers).unwrap(),
        )
        .unwrap();
        let listing = containers
            .as_array()
            .unwrap()
            .iter()
            .map(|container| {
                format!(
                    "{} {}\n",
                    container["Id"].as_str().unwrap(),
                    container["Config"]["Labels"]["com.docker.compose.project"]
                        .as_str()
                        .unwrap()
                )
            })
            .collect::<String>();
        fs::write(self.directory.path().join("ps"), listing).unwrap();
    }

    /// Requests a browser view from a disposable local dashboard process.
    fn page(&self, path: &str) -> String {
        let child = self
            .command()
            .args(["observe", "--port", "0", "--refresh", "30"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut server = Server(child);
        let mut line = String::new();
        BufReader::new(server.0.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(
            line.starts_with("http://127.0.0.1:"),
            "missing loopback URL: {line}"
        );
        let address = line
            .trim()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/');
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
}

struct Server(Child);

impl Drop for Server {
    /// Stops the local dashboard even when a browser assertion fails.
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Proves the live service contract preserves ports and excludes secrets and stopped metrics.
#[test]
fn status_json_reports_live_services_without_resolving_local_compose() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let snapshot: Value = serde_json::from_str(&text).unwrap();
    let app = &snapshot["apps"][0];
    assert_eq!(app["name"], "web");
    assert_eq!(app["configured"], true);
    let web = &app["services"][0];
    assert_eq!(web["service"], "web");
    assert_eq!(web["domains"][0], "app.example.com");
    assert_eq!(web["ports"][0]["internal"], "3000/tcp");
    assert_eq!(web["ports"][0]["published"][0], "127.0.0.1:8080");
    assert_eq!(web["health"], "healthy");
    assert_eq!(web["restart_count"], 2);
    assert!(web["uptime_seconds"].as_u64().unwrap() > 0);
    assert_eq!(web["memory"], "4MiB / 1GiB");
    assert_eq!(web["cpu"], "0.25%");
    assert!(app["services"][1]["memory"].is_null());
    assert!(app["services"][1]["uptime_seconds"].is_null());
    assert!(!text.contains("must-never-appear"));
}

/// Keeps human output useful for the domain and port question.
#[test]
fn status_table_shows_domains_ports_and_resource_usage() {
    let fixture = Fixture::new();
    let output = fixture.command().arg("status").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "APP",
        "SERVICE",
        "HOST PORTS",
        "app.example.com",
        "3000/tcp",
        "127.0.0.1:8080",
        "4MiB / 1GiB",
        "0.25%",
        "exited",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
}

/// Makes an unreachable Docker host fail explicitly instead of displaying an empty healthy host.
#[test]
fn status_surfaces_docker_failures() {
    let fixture = Fixture::new();
    fs::write(
        fixture.directory.path().join("docker"),
        "#!/bin/sh\necho 'Docker host unavailable' >&2\nexit 1\n",
    )
    .unwrap();
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Docker host unavailable"));
    assert!(output.stdout.is_empty());
}

/// Keeps operational visibility available when a deployed service has an unsupported route.
#[test]
fn invalid_live_routes_do_not_hide_container_status() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("inspect.json");
    let details = fs::read_to_string(&path)
        .unwrap()
        .replace("Host(`app.example.com`)", "HostRegexp(`.+`)");
    fs::write(path, details).unwrap();
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(snapshot["apps"][0]["services"].as_array().unwrap().len(), 2);
    let service = &snapshot["apps"][0]["services"][0];
    assert_eq!(service["state"], "running");
    assert!(service["domains"].as_array().unwrap().is_empty());
    assert!(
        service["routing_error"]
            .as_str()
            .unwrap()
            .contains("HostRegexp")
    );
}

/// Preserves failed-job evidence so a running web process cannot mask a failed supporting service.
#[test]
fn failed_container_exit_code_reaches_status_and_dashboard() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("inspect.json");
    let mut containers: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    containers[1]["State"]["ExitCode"] = 1.into();
    fs::write(path, serde_json::to_vec(&containers).unwrap()).unwrap();
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(snapshot["apps"][0]["services"][1]["exit_code"], 1);
    let response = fixture.page("/");
    assert!(
        response.contains("Needs attention"),
        "failed service did not affect deployment health"
    );
    assert!(
        response.contains("Failed"),
        "failed service was not identified"
    );
}

/// Keeps successful one-off setup jobs from creating false deployment alarms.
#[test]
fn successful_jobs_are_completed_without_flagging_a_running_deployment() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("inspect.json");
    let mut containers: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    containers[1]["State"]["ExitCode"] = 0.into();
    fs::write(path, serde_json::to_vec(&containers).unwrap()).unwrap();
    let response = fixture.page("/");
    assert!(
        response.contains("Completed"),
        "successful job was not distinguished from a stopped service"
    );
    assert!(
        !response.contains("Needs attention"),
        "successful setup job raised an alarm"
    );
}

/// Prevents a deliberately persistent service from being mislabeled as a successful setup job.
#[test]
fn stopped_persistent_services_are_not_completed_jobs() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("inspect.json");
    let mut containers: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    containers[1]["State"]["ExitCode"] = 0.into();
    containers[1]["HostConfig"] = serde_json::json!({"RestartPolicy": {"Name": "unless-stopped"}});
    fs::write(path, serde_json::to_vec(&containers).unwrap()).unwrap();
    let response = fixture.page("/");
    assert!(
        response.contains("Partially running"),
        "stopped persistent service was counted as a completed job"
    );
    assert!(!response.contains("Completed"));
}

/// Protects the local browser view from executing metadata or publishing credentials.
#[test]
fn observe_serves_escaped_status_on_loopback() {
    let fixture = Fixture::new();
    let response = fixture.page("/");
    for expected in [
        "200 OK",
        "no-store",
        "content=\"30\"",
        "app.example.com",
        "127.0.0.1:8080",
        "worker&lt;script&gt;",
    ] {
        assert!(
            response.contains(expected),
            "missing {expected}: {response}"
        );
    }
    assert!(!response.contains("worker<script>"));
    assert!(!response.contains("must-never-appear"));
}

/// Links known local web bindings while leaving database ports and stopped services unlinked.
#[test]
fn local_web_bindings_become_browser_links() {
    let fixture = Fixture::new();
    let mut containers: Value =
        serde_json::from_slice(&fs::read(fixture.directory.path().join("inspect.json")).unwrap())
            .unwrap();
    containers[0]["Config"]["Labels"]["traefik.enable"] = "false".into();
    containers[0]["NetworkSettings"]["Ports"]["5432/tcp"] = serde_json::json!([
        {"HostIp": "127.0.0.1", "HostPort": "5432"}
    ]);
    containers[1]["NetworkSettings"]["Ports"] = serde_json::json!({
        "80/tcp": [{"HostIp": "127.0.0.1", "HostPort": "8081"}]
    });
    fixture.containers(&containers);
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        snapshot["apps"][0]["services"][0]["links"],
        serde_json::json!(["http://127.0.0.1:8080/"])
    );
    assert_eq!(
        snapshot["apps"][0]["services"][1]["links"],
        serde_json::json!([])
    );
    for path in ["/", "/?inspect=0"] {
        let html = fixture.page(path);
        assert!(
            html.contains("href=\"http://127.0.0.1:8080/\""),
            "missing local app link"
        );
        assert!(!html.contains("href=\"http://127.0.0.1:5432/\""));
    }
}

/// Resolves routed localhost domains through the matching proxy's published HTTP and TLS ports.
#[test]
fn route_links_use_the_matching_proxy_host_ports() {
    let fixture = Fixture::new();
    let mut containers: Value =
        serde_json::from_slice(&fs::read(fixture.directory.path().join("inspect.json")).unwrap())
            .unwrap();
    containers[0]["Config"]["Labels"] = serde_json::json!({
        "com.docker.compose.project": "rootstock-web",
        "com.docker.compose.service": "web",
        "traefik.enable": "true",
        "traefik.http.routers.web.rule": "Host(`preview.localhost`)",
        "traefik.http.routers.web.entrypoints": "web",
        "traefik.http.routers.secure.rule": "Host(`secure.localhost`)",
        "traefik.http.routers.secure.entrypoints": "websecure",
        "traefik.http.routers.secure.tls": "true"
    });
    containers[0]["NetworkSettings"] = serde_json::json!({"Ports": {}, "Networks": {"edge": {}}});
    containers.as_array_mut().unwrap().push(serde_json::json!({
        "Id": "proxy-id", "Name": "/example-proxy", "RestartCount": 0,
        "Config": {"Labels": {"com.docker.compose.project": "rootstock-proxy", "com.docker.compose.service": "traefik"},
            "Cmd": ["--providers.docker=true", "--providers.docker.network=edge", "--entrypoints.web.address=:80", "--entrypoints.websecure.address=:443"]},
        "State": {"Status": "running", "Running": true, "StartedAt": "2026-01-01T00:00:00Z"},
        "NetworkSettings": {"Networks": {"edge": {}}, "Ports": {
            "80/tcp": [{"HostIp": "127.0.0.1", "HostPort": "32772"}],
            "443/tcp": [{"HostIp": "127.0.0.1", "HostPort": "8443"}]
        }}
    }));
    fixture.containers(&containers);
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
    let service = &snapshot["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["name"] == "web")
        .unwrap()["services"][0];
    assert_eq!(
        service["links"],
        serde_json::json!([
            "http://preview.localhost:32772/",
            "https://secure.localhost:8443/"
        ])
    );
    let html = fixture.page("/");
    assert!(html.contains("href=\"http://preview.localhost:32772/\""));
    assert!(html.contains("href=\"https://secure.localhost:8443/\""));
}

/// Keeps remote loopback bindings private and uses explicit TLS metadata for public links.
#[test]
fn remote_hosts_never_generate_client_loopback_links() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("inspect.json");
    let mut containers: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    containers[0]["Config"]["Labels"]["traefik.http.routers.web.tls.certresolver"] =
        "rootstock".into();
    containers[0]["Config"]["Labels"]["traefik.http.routers.local.rule"] =
        "Host(`preview.LOCALHOST`) || Host(`127.0.0.1`)".into();
    fixture.containers(&containers);
    for context in [false, true] {
        let mut command = fixture.command();
        if context {
            command
                .env("DOCKER_HOST", "unix:///example/docker.sock")
                .env("DOCKER_CONTEXT", "remote")
                .env("FIXTURE_DOCKER_ENDPOINT", "ssh://deploy@host.example.test");
        } else {
            command.env("DOCKER_HOST", "ssh://deploy@host.example.test");
        }
        let output = command.args(["status", "--json"]).output().unwrap();
        assert!(output.status.success());
        let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            snapshot["apps"][0]["services"][0]["links"],
            serde_json::json!(["https://app.example.com/"])
        );
    }
}

/// Rejects unsafe hostname metadata and formats IPv6 web bindings as valid browser URLs.
#[test]
fn link_metadata_cannot_inject_url_authority_or_markup() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("inspect.json");
    let mut containers: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    containers[0]["Config"]["Labels"]["traefik.http.routers.web.rule"] =
        "Host(`safe.example.com@evil.example.test`)".into();
    containers[0]["NetworkSettings"]["Ports"] = serde_json::json!({
        "3000/tcp": [{"HostIp": "::1", "HostPort": "8080"}],
        "5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "5432"}]
    });
    fixture.containers(&containers);
    let output = fixture
        .command()
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let snapshot: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        snapshot["apps"][0]["services"][0]["links"],
        serde_json::json!(["http://[::1]:8080/"])
    );
    let html = fixture.page("/");
    assert!(html.contains("href=\"http://[::1]:8080/\""));
    assert!(!html.contains("href=\"http://safe.example.com@evil.example.test/\""));
}
