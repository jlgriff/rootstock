use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::Path;

use anyhow::{Context, Result};
use tiny_http::{Header, Method, Response, Server};

use crate::status::{self, AppStatus, ServiceStatus, Snapshot};

const LOGO: &str = include_str!("logo.svg");

/// Escapes container metadata before inserting it into browser markup.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Supplies small inline icons without fonts, scripts, or external requests.
fn icon(name: &str) -> String {
    let path = match name {
        "refresh" => {
            r#"<path d="M20 7v5h-5M4 17v-5h5"/><path d="M6 7a7 7 0 0 1 12-1l2 6M4 12l2 6a7 7 0 0 0 12-1"/>"#
        }
        "stack" => {
            r#"<rect x="4" y="3" width="16" height="7" rx="2"/><rect x="4" y="14" width="16" height="7" rx="2"/><path d="M8 6.5h.01M8 17.5h.01"/>"#
        }
        "chevron" => r#"<path d="m9 5 7 7-7 7"/>"#,
        "check" => r#"<path d="m5 12 4 4L19 6"/>"#,
        "alert" => r#"<path d="m12 3 10 18H2L12 3Z"/><path d="M12 9v4m0 4h.01"/>"#,
        "external" => {
            r#"<path d="M14 3h7v7m0-7L10 14"/><path d="M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5"/>"#
        }
        _ => r#"<rect x="5" y="3" width="14" height="18" rx="2"/><path d="M9 7h6m-6 5h6m-6 5h3"/>"#,
    };
    format!(
        r#"<svg class="inline-block size-[18px] shrink-0 align-middle" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{path}</svg>"#
    )
}

/// Frames the console with shared navigation and a refresh action that retains inspection state.
fn document(content: &str, refresh: u64) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="{refresh}"><title>Overview · Rootstock</title><link rel="icon" type="image/svg+xml" href="/favicon.svg"><style>{}</style></head>
<body class="bg-canvas font-sans text-ink antialiased"><header class="h-16 border-b border-slate-200 bg-white sm:h-18"><div class="mx-auto flex h-full max-w-[1248px] items-center justify-between gap-4 px-5 sm:px-6 lg:px-10"><a class="flex min-w-0 items-center gap-2.5 text-[19px] font-semibold tracking-[-0.7px] no-underline sm:text-[21px]" href="/"><span class="grid size-8 shrink-0 place-items-center">{}</span>rootstock<span class="ml-2 hidden border-l border-slate-200 pl-5 text-[13px] font-normal tracking-normal text-muted sm:inline">Console</span></a><span class="flex shrink-0 items-center gap-2 text-[10px] text-muted sm:text-xs [&>span]:size-1.5 [&>span]:rounded-full [&>span]:bg-slate-400"><span></span>Read-only session</span></div></header>
<main class="mx-auto min-w-0 max-w-[1248px] px-5 pt-7 pb-6 sm:px-6 lg:px-10 lg:pt-10"><div class="mb-8 flex flex-col items-start justify-between gap-5 lg:flex-row lg:items-end"><div><p class="mb-3.5 text-xs text-muted [&>span]:px-2.5 [&>span]:text-slate-400">Workspace <span>/</span> Overview</p><h1 class="text-[29px] leading-tight font-semibold tracking-tight sm:text-[32px]">Overview</h1><p class="mt-2.5 text-xs text-muted sm:text-sm">Deployment health, routes, and resource usage.</p></div>
<div class="flex w-full items-center justify-between gap-4 lg:w-auto"><span class="flex items-center gap-2 whitespace-nowrap text-xs text-muted"><span class="inline-block size-1.5 shrink-0 rounded-full bg-emerald-700 ring-[3px] ring-emerald-50"></span>Updates every {refresh}s</span><a href="" class="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg border border-slate-300 bg-white px-3.5 py-2.5 text-xs font-medium shadow-xs transition hover:border-slate-400 hover:bg-slate-50 [&>svg]:size-3.5 [&>svg]:text-muted">{}Refresh</a></div></div>
{content}<footer class="mt-10 flex items-start justify-between gap-6 border-t border-slate-200 pt-5 text-[10px] leading-relaxed text-muted lg:mt-12 [&>span:first-child]:max-w-[260px]"><span>Health reflects container checks. Public routes are not probed.</span><span>Rootstock console</span></footer></main></body></html>"#,
        include_str!("dashboard.css"),
        LOGO,
        icon("refresh")
    )
}

/// Describes reported container health without treating successful setup jobs as failures.
fn service_health(service: &ServiceStatus) -> (&'static str, &'static str) {
    match (
        service.state.as_str(),
        service.health.as_deref(),
        service.exit_code,
    ) {
        ("running", Some("unhealthy"), _) | ("dead", _, _) => ("Unhealthy", "danger"),
        ("exited", _, Some(code)) if code != 0 => ("Failed", "danger"),
        ("restarting", _, _) => ("Restarting", "warning"),
        ("running", Some("starting"), _) => ("Starting", "warning"),
        _ if service.routing_error.is_some() => ("Check routing", "warning"),
        ("running", Some("healthy"), _) => ("Healthy", "success"),
        ("running", _, _) => ("Running", "success"),
        ("exited", _, Some(0))
            if matches!(
                service.restart_policy.as_deref(),
                Some("always" | "unless-stopped")
            ) =>
        {
            ("Stopped", "neutral")
        }
        ("exited", _, Some(0)) => ("Completed", "neutral"),
        ("paused", _, _) => ("Paused", "neutral"),
        ("created", _, _) => ("Not started", "neutral"),
        _ => ("Stopped", "neutral"),
    }
}

/// Rolls service states into an actionable deployment status.
fn deployment_health(app: &AppStatus) -> (&'static str, &'static str) {
    if app.services.is_empty() {
        return ("Not deployed", "neutral");
    }
    let states: Vec<_> = app.services.iter().map(service_health).collect();
    if states.iter().any(|(_, tone)| *tone == "danger") {
        return ("Needs attention", "danger");
    }
    if states.iter().any(|(_, tone)| *tone == "warning") {
        return ("Check services", "warning");
    }
    if states.iter().all(|(label, _)| *label == "Completed") {
        return ("Completed", "neutral");
    }
    if app
        .services
        .iter()
        .any(|service| service.state == "running")
    {
        if states
            .iter()
            .any(|(label, _)| matches!(*label, "Stopped" | "Not started" | "Paused"))
        {
            return ("Partially running", "warning");
        }
        return ("Running", "success");
    }
    ("Stopped", "neutral")
}

/// Converts Docker's formatted memory sample into bytes without interpreting its limit as usage.
fn memory_bytes(sample: &str) -> Option<f64> {
    let usage = sample.split(" / ").next()?;
    for (unit, multiplier) in [
        ("TiB", 1024_f64.powi(4)),
        ("GiB", 1024_f64.powi(3)),
        ("MiB", 1024_f64.powi(2)),
        ("KiB", 1024.0),
        ("B", 1.0),
    ] {
        if let Some(value) = usage.strip_suffix(unit) {
            return Some(value.trim().parse::<f64>().ok()? * multiplier);
        }
    }
    None
}

/// Aggregates running-container samples while keeping incomplete totals unavailable.
fn usage<'a>(services: impl Iterator<Item = &'a ServiceStatus>) -> (Option<f64>, Option<f64>) {
    let running: Vec<_> = services
        .filter(|service| service.state == "running")
        .collect();
    if running.is_empty() {
        return (None, None);
    }
    let memory = running.iter().try_fold(0.0, |sum, service| {
        Some(sum + memory_bytes(service.memory.as_deref()?)?)
    });
    let cpu = running.iter().try_fold(0.0, |sum, service| {
        Some(
            sum + service
                .cpu
                .as_deref()?
                .strip_suffix('%')?
                .parse::<f64>()
                .ok()?,
        )
    });
    (memory, cpu)
}

/// Formats real memory usage at a useful scale for a small server.
fn memory_label(bytes: Option<f64>) -> String {
    match bytes {
        Some(bytes) if bytes >= 1024_f64.powi(3) => format!("{:.1} GiB", bytes / 1024_f64.powi(3)),
        Some(bytes) => format!("{:.1} MiB", bytes / 1024_f64.powi(2)),
        None => "—".to_string(),
    }
}

/// Shows a readable UTC update time while retaining the precise timestamp in markup.
fn updated_at(snapshot: &Snapshot) -> String {
    let display = time::OffsetDateTime::parse(
        &snapshot.captured_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map(|time| {
        format!(
            "{:02}:{:02}:{:02} UTC",
            time.hour(),
            time.minute(),
            time.second()
        )
    })
    .unwrap_or_else(|_| snapshot.captured_at.clone());
    format!(
        r#"<time datetime="{}">{}</time>"#,
        escape(&snapshot.captured_at),
        escape(&display)
    )
}

/// Renders a labelled state with a text equivalent for its color indicator.
fn badge(label: &str, tone: &str) -> String {
    let colors = match tone {
        "success" => "bg-emerald-50 text-emerald-800",
        "warning" => "bg-amber-50 text-amber-900",
        "danger" => "bg-red-50 text-red-800",
        _ => "bg-slate-100 text-slate-600",
    };
    format!(
        r#"<span class="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md px-2 py-1.5 text-[10px] leading-none font-medium sm:text-[11px] [&>span]:size-1.25 [&>span]:shrink-0 [&>span]:rounded-full [&>span]:bg-current {colors}"><span></span>{}</span>"#,
        escape(label)
    )
}

/// Presents shared endpoint URLs as escaped links that preserve the console in its current tab.
fn browser_links<'a>(links: impl Iterator<Item = &'a String>) -> String {
    let links: std::collections::BTreeSet<_> = links.collect();
    links.into_iter().map(|url| format!(
        r#"<a class="inline-flex max-w-full items-center gap-1 text-brand underline decoration-brand/30 underline-offset-2 hover:decoration-brand focus-visible:outline-2 focus-visible:outline-offset-2 [&>svg]:size-3" href="{}" target="_blank" rel="noopener noreferrer"><span class="min-w-0 wrap-anywhere">{}</span>{}</a>"#,
        escape(url), escape(url), icon("external")
    )).collect::<Vec<_>>().join("<br>")
}

/// Keeps the full container inventory available behind a deployment's inspection control.
fn service_details(app: &AppStatus, index: usize, open: bool) -> String {
    let hidden = if open { "" } else { " hidden" };
    let source = if app.configured {
        "Local definition available"
    } else {
        "Discovered on selected host"
    };
    let mut content = format!(
        r#"<div class="min-w-0 border-t border-slate-200" id="inspection-{index}"{hidden}><div class="flex items-center justify-between gap-5 bg-slate-50 px-5 py-4 lg:px-6 [&_h3]:text-xs [&_h3]:font-semibold [&_h3]:text-slate-600 [&_p]:mt-1.5 [&_p]:text-[11px] [&_p]:text-muted [&_p]:wrap-anywhere [&_p>span]:px-1.5"><div><h3>Containers</h3><p>{} <span>·</span> {source}</p></div><span class="inline-flex h-5 min-w-5 items-center justify-center rounded-md bg-slate-100 px-1.5 text-[11px] font-medium text-muted">{}</span></div>"#,
        escape(&app.project),
        app.services.len()
    );
    if app.services.is_empty() {
        content.push_str(r#"<p class="px-6 pt-3 pb-6 text-xs text-muted">No containers have been deployed for this application.</p></div>"#);
        return content;
    }
    content.push_str(r#"<div class="max-w-full overflow-x-auto"><table class="w-full border-collapse text-left text-xs [&_th]:border-t [&_th]:border-slate-100 [&_th]:bg-slate-50 [&_th]:px-5 [&_th]:py-3 [&_th]:text-[10px] [&_th]:font-medium [&_th]:whitespace-nowrap [&_th]:text-muted [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-5 [&_td]:py-3.5 [&_td]:align-top [&_td]:text-slate-600 [&_td>strong]:block [&_td>strong]:font-medium [&_td>strong]:text-slate-700 [&_td_small]:mt-1.5 [&_td_small]:block [&_td_small]:text-[10px] [&_td_small]:text-muted [&_td_small]:wrap-anywhere"><thead><tr><th>Service / container</th><th>Status</th><th>Ports &amp; routes</th><th>Uptime</th><th>Restarts</th><th>Memory / limit</th><th>CPU</th></tr></thead><tbody>"#);
    for service in &app.services {
        let (label, tone) = service_health(service);
        let health = match service.exit_code {
            Some(code) => format!("Exit code {code}"),
            None => service
                .health
                .clone()
                .unwrap_or_else(|| "No healthcheck".to_string()),
        };
        let internal = service.internal_ports();
        let published = service.host_ports();
        let mut ports = format!(
            "<span>{}</span>",
            escape(if internal.is_empty() {
                "No declared ports"
            } else {
                &internal
            })
        );
        if !published.is_empty() {
            ports.push_str(&format!("<small>Host: {}</small>", escape(&published)));
        }
        let domains = service.domain_status();
        if !domains.is_empty() {
            ports.push_str(&format!("<small>{}</small>", escape(&domains)));
        }
        let links = browser_links(service.links.iter());
        if !links.is_empty() {
            ports.push_str(&format!("<small>{links}</small>"));
        }
        content.push_str(&format!(r#"<tr><td><strong>{}</strong><small class="max-w-[250px] wrap-anywhere">{}</small></td><td>{}<small>{}</small></td><td>{ports}</td><td class="whitespace-nowrap tabular-nums">{}</td><td class="whitespace-nowrap tabular-nums">{}</td><td class="whitespace-nowrap tabular-nums">{}</td><td class="whitespace-nowrap tabular-nums">{}</td></tr>"#,
            escape(&service.service), escape(&service.container), badge(label, tone), escape(&health),
            status::uptime(service.uptime_seconds), service.restart_count,
            escape(service.memory.as_deref().unwrap_or("Unavailable")), escape(service.cpu.as_deref().unwrap_or("Unavailable"))));
    }
    content.push_str("</tbody></table></div></div>");
    content
}

/// Presents deployment health first and preserves expanded inspection across automatic refreshes.
fn render(snapshot: &Snapshot, refresh: u64, inspect: Option<usize>) -> String {
    let total: usize = snapshot.apps.iter().map(|app| app.services.len()).sum();
    let running = snapshot
        .apps
        .iter()
        .flat_map(|app| &app.services)
        .filter(|service| service.state == "running")
        .count();
    let attention = snapshot
        .apps
        .iter()
        .filter(|app| matches!(deployment_health(app).1, "danger" | "warning"))
        .count();
    let (memory, cpu) = usage(snapshot.apps.iter().flat_map(|app| &app.services));
    let cpu = cpu
        .map(|value| format!("{value:.2}%"))
        .unwrap_or_else(|| "—".to_string());
    let mut content = format!(
        r#"<section class="grid min-w-0 grid-cols-2 overflow-hidden rounded-xl border border-slate-200 bg-white shadow-xs lg:grid-cols-4" aria-label="Server summary">
<div class="min-w-0 border-r border-b border-slate-200 px-4 py-5 even:border-r-0 last:border-b-0 [&:nth-last-child(2)]:border-b-0 lg:border-b-0 lg:px-6 lg:even:border-r lg:last:border-r-0 [&>p]:mb-3 [&>p]:text-xs [&>p]:font-medium [&>p]:text-muted [&>strong]:block [&>strong]:text-[25px] [&>strong]:leading-tight [&>strong]:font-semibold [&>strong]:tracking-tight [&>strong]:tabular-nums sm:[&>strong]:text-3xl [&>strong>span]:text-xl [&>strong>span]:font-normal [&>strong>span]:text-muted [&>small]:mt-2 [&>small]:block [&>small]:text-[10px] [&>small]:leading-relaxed [&>small]:text-muted lg:[&>small]:text-[11px]"><p>Deployments</p><strong>{}</strong><small>Apps and shared services</small></div>
<div class="min-w-0 border-r border-b border-slate-200 px-4 py-5 even:border-r-0 last:border-b-0 [&:nth-last-child(2)]:border-b-0 lg:border-b-0 lg:px-6 lg:even:border-r lg:last:border-r-0 [&>p]:mb-3 [&>p]:text-xs [&>p]:font-medium [&>p]:text-muted [&>strong]:block [&>strong]:text-[25px] [&>strong]:leading-tight [&>strong]:font-semibold [&>strong]:tracking-tight [&>strong]:tabular-nums sm:[&>strong]:text-3xl [&>strong>span]:text-xl [&>strong>span]:font-normal [&>strong>span]:text-muted [&>small]:mt-2 [&>small]:block [&>small]:text-[10px] [&>small]:leading-relaxed [&>small]:text-muted lg:[&>small]:text-[11px]"><p>Running services</p><strong>{running}<span> / {total}</span></strong><small>Across all deployments</small></div>
<div class="min-w-0 border-r border-b border-slate-200 px-4 py-5 even:border-r-0 last:border-b-0 [&:nth-last-child(2)]:border-b-0 lg:border-b-0 lg:px-6 lg:even:border-r lg:last:border-r-0 [&>p]:mb-3 [&>p]:text-xs [&>p]:font-medium [&>p]:text-muted [&>strong]:block [&>strong]:text-[25px] [&>strong]:leading-tight [&>strong]:font-semibold [&>strong]:tracking-tight [&>strong]:tabular-nums sm:[&>strong]:text-3xl [&>strong>span]:text-xl [&>strong>span]:font-normal [&>strong>span]:text-muted [&>small]:mt-2 [&>small]:block [&>small]:text-[10px] [&>small]:leading-relaxed [&>small]:text-muted lg:[&>small]:text-[11px]"><p>Container memory</p><strong class="text-[25px]! sm:text-[28px]!">{}</strong><small>Current running-container usage</small></div>
<div class="min-w-0 border-r border-b border-slate-200 px-4 py-5 even:border-r-0 last:border-b-0 [&:nth-last-child(2)]:border-b-0 lg:border-b-0 lg:px-6 lg:even:border-r lg:last:border-r-0 [&>p]:mb-3 [&>p]:text-xs [&>p]:font-medium [&>p]:text-muted [&>strong]:block [&>strong]:text-[25px] [&>strong]:leading-tight [&>strong]:font-semibold [&>strong]:tracking-tight [&>strong]:tabular-nums sm:[&>strong]:text-3xl [&>strong>span]:text-xl [&>strong>span]:font-normal [&>strong>span]:text-muted [&>small]:mt-2 [&>small]:block [&>small]:text-[10px] [&>small]:leading-relaxed [&>small]:text-muted lg:[&>small]:text-[11px]"><p>CPU usage</p><strong class="text-[25px]! sm:text-[28px]!">{cpu}</strong><small>Combined container CPU</small></div></section>"#,
        snapshot.apps.len(),
        memory_label(memory)
    );
    if attention > 0 {
        let noun = if attention == 1 {
            "deployment"
        } else {
            "deployments"
        };
        content.push_str(&format!(r#"<div class="mt-6 flex items-center gap-3 rounded-lg border border-orange-200 bg-orange-50/60 px-5 py-4 text-orange-900 [&>svg]:size-5 [&_strong]:text-xs [&_strong]:font-semibold [&_p]:mt-1 [&_p]:text-xs [&_p]:text-orange-800">{}<div><strong>{attention} {noun} to review</strong><p>Check the flagged services and routing details below.</p></div></div>"#, icon("alert")));
    }
    let health = if attention == 0 {
        format!(
            r#"<span class="flex shrink-0 items-center gap-1 whitespace-nowrap pt-1 text-[10px] text-emerald-700 sm:gap-1.5 sm:text-[11px] [&>svg]:size-3 sm:[&>svg]:size-3.5">{}No reported failures</span>"#,
            icon("check")
        )
    } else {
        format!(r#"<span class="shrink-0 text-xs text-orange-800">{attention} to review</span>"#)
    };
    content.push_str(&format!(r#"<div class="mt-8 mb-4 flex min-w-0 items-start justify-between gap-4 lg:mt-9 lg:items-center [&>div]:min-w-0 [&_h2]:flex [&_h2]:items-center [&_h2]:gap-2 [&_h2]:text-base [&_h2]:font-semibold [&_h2]:tracking-tight sm:[&_h2]:text-[17px] [&_h2>span]:inline-flex [&_h2>span]:h-5 [&_h2>span]:min-w-5 [&_h2>span]:items-center [&_h2>span]:justify-center [&_h2>span]:rounded-md [&_h2>span]:bg-slate-100 [&_h2>span]:px-1.5 [&_h2>span]:text-[11px] [&_h2>span]:font-medium [&_h2>span]:tracking-normal [&_h2>span]:text-muted [&_p]:mt-1.5 [&_p]:text-[11px] [&_p]:leading-relaxed [&_p]:text-muted sm:[&_p]:text-xs"><div><h2>Deployments <span>{}</span></h2><p>Applications and the services that support them.</p></div>{health}</div><div class="grid min-w-0 gap-3">"#, snapshot.apps.len()));
    for (index, app) in snapshot.apps.iter().enumerate() {
        let (label, tone) = deployment_health(app);
        let (memory, cpu) = usage(app.services.iter());
        let cpu = cpu
            .map(|value| format!("{value:.2}%"))
            .unwrap_or_else(|| "—".to_string());
        let running = app
            .services
            .iter()
            .filter(|service| service.state == "running")
            .count();
        let completed = app
            .services
            .iter()
            .filter(|service| service_health(service).0 == "Completed")
            .count();
        let domains: std::collections::BTreeSet<_> = app
            .services
            .iter()
            .flat_map(|service| &service.domains)
            .collect();
        let mut endpoint = browser_links(app.services.iter().flat_map(|service| &service.links));
        if endpoint.is_empty() {
            endpoint = escape(&if domains.is_empty() {
                "No domain assigned".to_string()
            } else {
                domains.into_iter().cloned().collect::<Vec<_>>().join(", ")
            });
        }
        let open = inspect == Some(index);
        let href = if open {
            format!("/#deployment-{index}")
        } else {
            format!("/?inspect={index}#deployment-{index}")
        };
        let action = if open { "Close details" } else { "Details" };
        let extra = if completed > 0 {
            format!(" · {completed} completed")
        } else {
            String::new()
        };
        content.push_str(&format!(r#"<article class="min-w-0 scroll-mt-6 overflow-hidden rounded-xl border border-slate-200 bg-white shadow-xs" id="deployment-{index}" aria-labelledby="deployment-name-{index}">
<div class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-4 gap-y-5 p-5 lg:grid-cols-[minmax(200px,1fr)_140px_110px_75px_100px] lg:gap-4 lg:p-6"><div class="col-span-full flex min-w-0 items-start gap-3 lg:col-span-1 lg:gap-4 [&>div]:min-w-0 [&_h3]:text-[15px] [&_h3]:font-semibold [&_h3]:tracking-tight [&_h3]:wrap-anywhere"><span class="grid size-[42px] shrink-0 place-items-center rounded-xl border border-emerald-100 bg-emerald-50/60 text-emerald-700 [&>svg]:size-[21px]">{}</span><div><h3 id="deployment-name-{index}">{}</h3><p class="mt-1.5 text-xs leading-relaxed text-muted wrap-anywhere">{}</p><p class="mt-1.5 text-[11px] leading-relaxed text-muted">{running} running · {} containers{extra}</p></div></div>
<div class="min-w-0 self-center">{}</div><div class="flex min-w-0 flex-col gap-2 [&>span]:text-[10px] [&>span]:text-muted sm:[&>span]:text-[11px] [&>strong]:whitespace-nowrap [&>strong]:text-xs [&>strong]:font-medium [&>strong]:text-slate-700 [&>strong]:tabular-nums sm:[&>strong]:text-[13px]"><span>Memory</span><strong>{}</strong></div><div class="flex min-w-0 flex-col gap-2 [&>span]:text-[10px] [&>span]:text-muted sm:[&>span]:text-[11px] [&>strong]:whitespace-nowrap [&>strong]:text-xs [&>strong]:font-medium [&>strong]:text-slate-700 [&>strong]:tabular-nums sm:[&>strong]:text-[13px]"><span>CPU</span><strong>{cpu}</strong></div>
<a class="col-span-full inline-flex items-center justify-between gap-2 whitespace-nowrap border-t border-slate-100 pt-4 text-xs font-medium text-muted hover:text-brand lg:col-span-1 lg:justify-end lg:border-0 lg:pt-0 [&>svg]:size-3 [&>svg]:text-muted aria-expanded:[&>svg]:rotate-90" href="{href}" aria-expanded="{open}" aria-controls="inspection-{index}">{action}{}</a></div>{}</article>"#,
            icon("stack"), escape(&app.name), endpoint, app.services.len(), badge(label, tone), memory_label(memory), icon("chevron"), service_details(app, index, open)));
    }
    if snapshot.apps.is_empty() {
        content.push_str(&format!(r#"<div class="rounded-xl border border-slate-200 bg-white px-6 py-14 text-center [&_h3]:text-[17px] [&_h3]:font-semibold [&_p]:mx-auto [&_p]:mt-3 [&_p]:mb-6 [&_p]:max-w-sm [&_p]:text-[13px] [&_p]:leading-relaxed [&_p]:text-muted [&_code]:rounded-md [&_code]:border [&_code]:border-slate-200 [&_code]:bg-slate-50 [&_code]:px-3.5 [&_code]:py-2.5 [&_code]:text-xs [&_code]:text-muted"><span class="mx-auto mb-5 grid size-12 place-items-center rounded-xl bg-emerald-50 text-emerald-700">{}</span><h3>Your first deployment starts here</h3><p>Deploy an app to this Docker host, then refresh to see its health and resource usage.</p><code>rootstock deploy &lt;name&gt;</code></div>"#, icon("stack")));
    }
    content.push_str(&format!(r#"</div><div class="mt-4 flex flex-wrap items-center justify-between gap-3 text-[10px] text-muted [&>span:first-child]:flex [&>span:first-child]:items-center [&>span:first-child]:gap-2"><span><span class="inline-block size-1.5 shrink-0 rounded-full bg-emerald-700 ring-[3px] ring-emerald-50"></span>Snapshot collected</span><span>Updated {}</span></div>"#, updated_at(snapshot)));
    document(&content, refresh)
}

/// Serves the local console with read-only collection and URL-backed inspection state.
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
        let inspect = request
            .url()
            .strip_prefix("/?inspect=")
            .and_then(|value| value.parse::<usize>().ok());
        let (code, body) = if host != address.to_string()
            && host != format!("localhost:{}", address.port())
        {
            (400, "Unexpected Host header".to_string())
        } else if request.method() != &Method::Get {
            (405, "Only GET is supported".to_string())
        } else if request.url() == "/favicon.svg" {
            (200, LOGO.to_string())
        } else if request.url() != "/" && inspect.is_none() {
            (404, "Not found".to_string())
        } else {
            match status::collect(root) {
                Ok(snapshot) => (200, render(&snapshot, refresh, inspect)),
                Err(error) => (
                    503,
                    document(
                        &format!(
                            r#"<section class="rounded-xl border border-slate-200 bg-white px-6 py-14 text-center [&_h2]:text-[17px] [&_h2]:font-semibold [&_p]:mx-auto [&_p]:mt-3 [&_p]:mb-6 [&_p]:max-w-sm [&_p]:text-[13px] [&_p]:leading-relaxed [&_p]:text-muted [&_details]:mx-auto [&_details]:max-w-xl [&_details]:text-xs [&_details]:text-muted [&_summary]:cursor-pointer"><span class="mx-auto mb-5 grid size-12 place-items-center rounded-xl bg-emerald-50 text-emerald-700">{}</span><h2>Server status is unavailable</h2><p>Check your Docker connection, then refresh to try again.</p><details><summary>Connection details</summary><pre class="mt-4 rounded-md border border-slate-100 bg-slate-50 p-4 text-left leading-relaxed whitespace-pre-wrap wrap-anywhere">{}</pre></details></section>"#,
                            icon("alert"),
                            escape(&format!("{error:#}"))
                        ),
                        refresh,
                    ),
                ),
            }
        };
        let content_type = if code == 200 && request.url() == "/favicon.svg" {
            "image/svg+xml"
        } else {
            "text/html; charset=utf-8"
        };
        let mut response = Response::from_string(body).with_status_code(code);
        for (name, value) in [
            ("Content-Type", content_type),
            ("Cache-Control", "no-store"),
            (
                "Content-Security-Policy",
                "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'",
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
