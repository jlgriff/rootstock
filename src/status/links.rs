use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use super::{Container, docker};
use crate::routes;

/// Determines whether published bindings refer to the browser's machine using Docker's precedence.
pub(super) fn local_engine() -> bool {
    let endpoint = if std::env::var("DOCKER_CONTEXT")
        .unwrap_or_default()
        .is_empty()
        && let Ok(host) = std::env::var("DOCKER_HOST")
        && !host.is_empty()
    {
        Some(host)
    } else {
        docker(&[
            "context",
            "inspect",
            "--format",
            "{{json .Endpoints.docker.Host}}",
        ])
        .ok()
        .and_then(|output| serde_json::from_slice::<String>(&output).ok())
    };
    endpoint.is_some_and(|endpoint| {
        endpoint.starts_with("unix://")
            || endpoint.starts_with("npipe://")
            || ["tcp://", "http://", "https://"].iter().any(|scheme| {
                endpoint.strip_prefix(scheme).is_some_and(|address| {
                    ["localhost:", "127.0.0.1:", "[::1]:"]
                        .iter()
                        .any(|host| address.starts_with(host))
                })
            })
    })
}

/// Reads a static Traefik command setting without interpreting shell syntax.
fn setting<'a>(container: &'a Container, name: &str) -> Option<&'a str> {
    container
        .config
        .cmd
        .as_ref()?
        .iter()
        .find_map(|argument| argument.strip_prefix(name)?.strip_prefix('='))
}

/// Identifies the Docker-discovering proxy so its generic listener is not offered as an app.
fn proxy(container: &Container) -> bool {
    setting(container, "--providers.docker") == Some("true")
}

/// Rejects hostname strings that could change a link's authority, path, or markup.
fn hostname(host: &str) -> bool {
    !host.is_empty()
        && host.trim_end_matches('.').split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

/// Preserves IPv6 brackets and omits a scheme's standard port.
fn url(scheme: &str, host: &str, port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let port = if (scheme, port) == ("http", 80) || (scheme, port) == ("https", 443) {
        String::new()
    } else {
        format!(":{port}")
    };
    format!("{scheme}://{host}{port}/")
}

/// Matches a proxy to the application's actual Docker network before using its published ports.
fn shares_network(container: &Container, proxy: &Container) -> bool {
    let Some(networks) = &container.network_settings.networks else {
        return false;
    };
    if let Some(network) = setting(proxy, "--providers.docker.network") {
        return networks.contains_key(network);
    }
    proxy
        .network_settings
        .networks
        .as_ref()
        .is_some_and(|other| networks.keys().any(|network| other.contains_key(network)))
}

/// Derives browser URLs from observed web bindings and explicit host routing without network probes.
pub(super) fn derive(container: &Container, containers: &[Container], local: bool) -> Vec<String> {
    if !container.state.running {
        return Vec::new();
    }
    let labels = container.config.labels.clone().unwrap_or_default();
    let mut links = BTreeSet::new();
    if local && !proxy(container) {
        let mut web_ports =
            BTreeMap::from([("80/tcp".into(), "http"), ("443/tcp".into(), "https")]);
        for (key, port) in &labels {
            if key.starts_with("traefik.http.services.")
                && let Some(prefix) = key.strip_suffix(".loadbalancer.server.port")
            {
                let scheme = labels
                    .get(&format!("{prefix}.loadbalancer.server.scheme"))
                    .map(String::as_str)
                    .unwrap_or("http");
                if matches!(scheme, "http" | "https") {
                    web_ports.insert(format!("{port}/tcp"), scheme);
                }
            }
        }
        for (internal, bindings) in container.network_settings.ports.iter().flatten() {
            let Some(scheme) = web_ports.get(internal) else {
                continue;
            };
            for binding in bindings.iter().flatten() {
                if let (Ok(ip), Ok(port)) = (
                    binding.address.parse::<IpAddr>(),
                    binding.port.parse::<u16>(),
                ) && port > 0
                {
                    let host = if ip.is_unspecified() {
                        "localhost".to_string()
                    } else {
                        ip.to_string()
                    };
                    links.insert(url(scheme, &host, port));
                }
            }
        }
    }
    if labels.get("traefik.enable").map(String::as_str) == Some("true") {
        for (key, rule) in &labels {
            if !key.starts_with("traefik.http.routers.") {
                continue;
            }
            let Some(prefix) = key.strip_suffix(".rule") else {
                continue;
            };
            let tls = labels.get(&format!("{prefix}.tls")).map(String::as_str) == Some("true")
                || labels
                    .keys()
                    .any(|key| key.starts_with(&format!("{prefix}.tls.")));
            let scheme = if tls { "https" } else { "http" };
            for host in routes::hostnames_in_rule(rule)
                .unwrap_or_default()
                .into_iter()
                .filter(|host| hostname(host))
                .map(|host| host.to_ascii_lowercase())
            {
                let loopback = host.trim_end_matches('.') == "localhost"
                    || host.trim_end_matches('.').ends_with(".localhost")
                    || host
                        .parse::<IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
                if !local && loopback {
                    continue;
                }
                let mut mapped = BTreeSet::new();
                for proxy in containers.iter().filter(|candidate| {
                    candidate.state.running
                        && proxy(candidate)
                        && shares_network(container, candidate)
                }) {
                    for entrypoint in labels
                        .get(&format!("{prefix}.entrypoints"))
                        .into_iter()
                        .flat_map(|value| value.split(',').map(str::trim))
                    {
                        let entrypoint = format!("--entrypoints.{entrypoint}");
                        let Some(port) = setting(proxy, &format!("{entrypoint}.address"))
                            .and_then(|address| address.rsplit(':').next())
                            .and_then(|port| port.parse::<u16>().ok())
                        else {
                            continue;
                        };
                        let scheme = if tls
                            || setting(proxy, &format!("{entrypoint}.http.tls")) == Some("true")
                            || setting(proxy, &format!("{entrypoint}.http.tls.certresolver"))
                                .is_some()
                        {
                            "https"
                        } else {
                            "http"
                        };
                        if let Some(bindings) = proxy
                            .network_settings
                            .ports
                            .as_ref()
                            .and_then(|ports| ports.get(&format!("{port}/tcp")))
                        {
                            for binding in bindings.iter().flatten() {
                                if let (Ok(ip), Ok(port)) = (
                                    binding.address.parse::<IpAddr>(),
                                    binding.port.parse::<u16>(),
                                ) && port > 0
                                    && (local || !ip.is_loopback())
                                {
                                    mapped.insert(url(scheme, &host, port));
                                }
                            }
                        }
                    }
                }
                if mapped.is_empty() && !loopback {
                    mapped.insert(url(scheme, &host, if tls { 443 } else { 80 }));
                }
                links.extend(mapped);
            }
        }
    }
    links.into_iter().collect()
}
