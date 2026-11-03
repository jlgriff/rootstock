use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::Path;

use anyhow::{Context, Result};
use tiny_http::{Header, Method, Response, Server};

use crate::status::{self, Snapshot};

/// Escapes container metadata before inserting it into browser markup.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Wraps a status snapshot or an error in a page that refreshes without JavaScript.
fn document(content: &str, refresh: u64) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="{refresh}"><title>Rootstock · Status</title><style>{}</style></head>
<body><main><header class="heading"><div><p class="eyebrow">Rootstock</p><h1>Application status</h1></div>
<a href="/" class="refresh">Refresh now</a></header>
<p class="intro">Live Docker state. Apps share public ports through the proxy; internal ports belong to each container.</p>
{content}<footer>Refreshes every {refresh} seconds. Current container health does not verify public reachability.</footer>
</main></body></html>"#,
        include_str!("dashboard.css")
    )
}

/// Displays the same selected runtime fields exposed by the status command.
fn render(snapshot: &Snapshot, refresh: u64) -> String {
    let total: usize = snapshot.apps.iter().map(|app| app.services.len()).sum();
    let running = snapshot
        .apps
        .iter()
        .flat_map(|app| &app.services)
        .filter(|service| service.state == "running")
        .count();
    let mut content = format!(
        r#"<div class="summary"><div><strong>{}</strong><span>Applications</span></div>
<div><strong>{running}</strong><span>Running containers</span></div><div><strong>{total}</strong><span>Total containers</span></div></div>
<p class="captured">Snapshot: {}</p><div class="table-wrap"><table><thead><tr>
<th>App</th><th>Service</th><th>State / health</th><th>Domains</th><th>Internal ports</th><th>Host ports</th>
<th>Uptime</th><th>Restarts</th><th>RAM / limit</th><th>CPU</th></tr></thead><tbody>"#,
        snapshot.apps.len(),
        escape(&snapshot.captured_at)
    );
    for app in &snapshot.apps {
        let registration = if app.configured {
            "In local inventory"
        } else {
            "Discovered on host"
        };
        let app_cell = format!(
            "<strong>{}</strong><small>{registration}</small>",
            escape(&app.name)
        );
        if app.services.is_empty() {
            content.push_str(&format!(
                "<tr><td>{app_cell}</td><td colspan=\"9\" class=\"muted\">Not deployed</td></tr>"
            ));
        }
        for service in &app.services {
            let state_class = match (service.state.as_str(), service.health.as_deref()) {
                ("running", Some("unhealthy")) | ("restarting", _) => "warning",
                ("running", _) => "running",
                _ => "muted",
            };
            content.push_str(&format!("<tr><td>{app_cell}</td><td><strong>{}</strong><small>{}</small></td><td><span class=\"{state_class}\">{}</span><small>{}</small></td>",
                escape(&service.service), escape(&service.container), escape(&service.state), escape(service.health.as_deref().unwrap_or("No healthcheck"))));
            let values = [
                service.domain_status(),
                service.internal_ports(),
                service.host_ports(),
                status::uptime(service.uptime_seconds),
                service.restart_count.to_string(),
                service
                    .memory
                    .clone()
                    .unwrap_or_else(|| "Unavailable".to_string()),
                service
                    .cpu
                    .clone()
                    .unwrap_or_else(|| "Unavailable".to_string()),
            ];
            for value in values {
                content.push_str(&format!(
                    "<td>{}</td>",
                    if value.is_empty() {
                        "—".to_string()
                    } else {
                        escape(&value)
                    }
                ));
            }
            content.push_str("</tr>");
        }
    }
    if snapshot.apps.is_empty() {
        content.push_str("<tr><td colspan=\"10\" class=\"empty\">No rootstock applications found on this host or in the local inventory.</td></tr>");
    }
    content.push_str("</tbody></table></div>");
    document(&content, refresh)
}

/// Serves an on-demand dashboard on loopback while using the existing Docker connection.
pub fn serve(root: &Path, port: u16, refresh: u64) -> Result<()> {
    let server = Server::http((Ipv4Addr::LOCALHOST, port)).map_err(anyhow::Error::msg)?;
    let address = server
        .server_addr()
        .to_ip()
        .context("reading dashboard address")?;
    println!("http://{address}/");
    io::stdout().flush()?;
    for request in server.incoming_requests() {
        let host = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Host"))
            .map(|header| header.value.as_str())
            .unwrap_or_default();
        let (code, body) = if host != address.to_string()
            && host != format!("localhost:{}", address.port())
        {
            (400, "Unexpected Host header".to_string())
        } else if request.method() != &Method::Get {
            (405, "Only GET is supported".to_string())
        } else if request.url() != "/" {
            (404, "Not found".to_string())
        } else {
            match status::collect(root) {
                Ok(snapshot) => (200, render(&snapshot, refresh)),
                Err(error) => (
                    503,
                    document(
                        &format!(
                            "<section class=\"error\"><h2>Status unavailable</h2><pre>{}</pre></section>",
                            escape(&format!("{error:#}"))
                        ),
                        refresh,
                    ),
                ),
            }
        };
        let mut response = Response::from_string(body).with_status_code(code);
        for (name, value) in [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Cache-Control", "no-store"),
            (
                "Content-Security-Policy",
                "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'",
            ),
            ("X-Content-Type-Options", "nosniff"),
        ] {
            response.add_header(Header::from_bytes(name, value).unwrap());
        }
        if let Err(error) = request.respond(response) {
            eprintln!("dashboard response failed: {error}");
        }
    }
    Ok(())
}
