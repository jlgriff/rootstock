use anyhow::{Context, Result, bail};
use serde_json::Value;

/// Marks a container as one Traefik should route to, since the proxy exposes nothing by default.
const ENABLE: &str = "traefik.enable";

/// The finite hostnames claimed by a rule whose every alternative is host-restricted.
pub fn hostnames_in_rule(rule: &str) -> Result<Vec<String>> {
    let mut parser = Rule {
        remaining: rule,
        hosts: Vec::new(),
    };
    let bounded = parser.expression()?;
    if !parser.remaining.trim().is_empty() {
        bail!("unexpected rule syntax: {}", parser.remaining);
    }
    if !bounded {
        bail!("`{rule}` has no host constraint on every alternative");
    }
    Ok(parser.hosts)
}

struct Rule<'a> {
    remaining: &'a str,
    hosts: Vec<String>,
}

impl Rule<'_> {
    /// Requires each alternative to restrict the possible request hostnames.
    fn expression(&mut self) -> Result<bool> {
        let mut bounded = self.conjunction()?;
        while self.consume("||") {
            let next = self.conjunction()?;
            bounded &= next;
        }
        Ok(bounded)
    }

    /// Lets a host constraint bound other matchers in the same conjunction.
    fn conjunction(&mut self) -> Result<bool> {
        let mut bounded = self.term()?;
        while self.consume("&&") {
            let next = self.term()?;
            bounded |= next;
        }
        Ok(bounded)
    }

    /// Reads a group or HTTP matcher without confusing quoted text with syntax.
    fn term(&mut self) -> Result<bool> {
        if self.consume("!") {
            bail!("negated matchers are unsupported; use a positive Host constraint");
        }
        if self.consume("(") {
            let bounded = self.expression()?;
            if !self.consume(")") {
                bail!("rule group is never closed");
            }
            return Ok(bounded);
        }
        let length = self
            .remaining
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(self.remaining.len());
        let name = &self.remaining[..length];
        self.remaining = &self.remaining[length..];
        if !self.consume("(") {
            bail!("expected an HTTP matcher");
        }
        let mut arguments = Vec::new();
        if !self.consume(")") {
            loop {
                arguments.push(self.argument()?);
                if self.consume(")") {
                    break;
                }
                if !self.consume(",") {
                    bail!("expected a comma or closing matcher parenthesis");
                }
            }
        }
        match name {
            "Host" => {
                if arguments.len() != 1 || arguments[0].trim().is_empty() {
                    bail!("Host requires exactly one host");
                }
                self.hosts.push(arguments.remove(0));
                Ok(true)
            }
            "Path" | "PathPrefix" | "PathRegexp" | "Method" | "Header" | "HeaderRegexp"
            | "Query" | "QueryRegexp" | "ClientIP" => Ok(false),
            _ => bail!(
                "unsupported HTTP matcher `{name}`; domains must use explicit Host constraints"
            ),
        }
    }

    /// Consumes a raw or escaped string while keeping its delimiters out of the grammar.
    fn argument(&mut self) -> Result<String> {
        self.remaining = self.remaining.trim_start();
        let bytes = self.remaining.as_bytes();
        let quote = match bytes.first() {
            Some(b'`' | b'"') => bytes[0],
            _ => bail!("matcher arguments must be quoted strings"),
        };
        let mut end = 1;
        while end < bytes.len() {
            if bytes[end] == quote {
                let literal = &self.remaining[..=end];
                let value = if quote == b'`' {
                    literal[1..end].to_string()
                } else {
                    serde_json::from_str(literal).context("unsupported quoted matcher argument")?
                };
                self.remaining = &self.remaining[end + 1..];
                return Ok(value);
            }
            end += if quote == b'"' && bytes[end] == b'\\' {
                2
            } else {
                1
            };
        }
        bail!("matcher string is never closed")
    }

    /// Skips whitespace and consumes one exact grammar token when present.
    fn consume(&mut self, token: &str) -> bool {
        self.remaining = self.remaining.trim_start();
        if let Some(rest) = self.remaining.strip_prefix(token) {
            self.remaining = rest;
            true
        } else {
            false
        }
    }
}

/// Every hostname a resolved Compose project claims, through the services that opt in.
pub fn domains_in_project(config: &Value) -> Result<Vec<String>> {
    let mut found = Vec::new();
    let Some(services) = config.get("services").and_then(Value::as_object) else {
        return Ok(found);
    };

    for (service, definition) in services {
        let Some(labels) = definition.get("labels").and_then(Value::as_object) else {
            continue;
        };
        if labels.get(ENABLE).and_then(Value::as_str) != Some("true") {
            continue;
        }

        let rules: Vec<&str> = labels
            .iter()
            .filter(|(key, _)| key.starts_with("traefik.http.routers.") && key.ends_with(".rule"))
            .filter_map(|(_, value)| value.as_str())
            .collect();

        if rules.is_empty() {
            bail!("`{service}` opts in to routing but declares no router rule");
        }

        for rule in rules {
            found.extend(hostnames_in_rule(rule).with_context(|| format!("service `{service}`"))?);
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds resolved Compose input without requiring a running Docker engine.
    fn project(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    const WEBSITE: &str = r#"{
      "services": {
        "website": {
          "labels": {
            "traefik.enable": "true",
            "traefik.http.routers.website.rule": "Host(`app.example.com`) || Host(`www.app.example.com`)"
          }
        }
      }
    }"#;

    #[test]
    /// Makes routing ownership available to conflict detection.
    fn reads_the_domains_a_service_opts_in_with() {
        assert_eq!(
            domains_in_project(&project(WEBSITE)).unwrap(),
            ["app.example.com", "www.app.example.com"]
        );
    }

    #[test]
    /// Keeps private services out of public routing inventory.
    fn ignores_a_service_that_never_opted_in() {
        let json = r#"{
          "services": {
            "litestream": { "labels": { "com.example.role": "backup" } }
          }
        }"#;

        assert_eq!(
            domains_in_project(&project(json)).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    /// Respects an explicit decision to keep a service private.
    fn ignores_a_service_that_opted_out() {
        let json = r#"{
          "services": {
            "engine": {
              "labels": {
                "traefik.enable": "false",
                "traefik.http.routers.engine.rule": "Host(`api.app.example.com`)"
              }
            }
          }
        }"#;

        assert_eq!(
            domains_in_project(&project(json)).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    /// Prevents alternate routes from escaping conflict checks.
    fn reads_every_router_on_one_service() {
        let json = r#"{
          "services": {
            "engine": {
              "labels": {
                "traefik.enable": "true",
                "traefik.http.routers.api.rule": "Host(`api.app.example.com`)",
                "traefik.http.routers.admin.rule": "Host(`admin.app.example.com`)"
              }
            }
          }
        }"#;

        let mut found = domains_in_project(&project(json)).unwrap();
        found.sort();
        assert_eq!(found, ["admin.app.example.com", "api.app.example.com"]);
    }

    #[test]
    /// Includes every public service in application ownership checks.
    fn reads_across_every_service_in_the_project() {
        let json = r#"{
          "services": {
            "website": {
              "labels": {
                "traefik.enable": "true",
                "traefik.http.routers.shop.rule": "Host(`app.example.com`)"
              }
            },
            "engine": {
              "labels": {
                "traefik.enable": "true",
                "traefik.http.routers.api.rule": "Host(`api.app.example.com`)"
              }
            }
          }
        }"#;

        let mut found = domains_in_project(&project(json)).unwrap();
        found.sort();
        assert_eq!(found, ["api.app.example.com", "app.example.com"]);
    }

    #[test]
    /// Makes unsupported routing declarations actionable.
    fn names_the_service_whose_rule_cannot_be_checked() {
        let json = r#"{
          "services": {
            "wildcard": {
              "labels": {
                "traefik.enable": "true",
                "traefik.http.routers.w.rule": "HostRegexp(`^.+[.]example[.]com$`)"
              }
            }
          }
        }"#;

        let error = format!("{:#}", domains_in_project(&project(json)).unwrap_err());

        assert!(error.contains("wildcard"), "{error}");
    }

    #[test]
    /// Prevents silent acceptance of an incomplete public route.
    fn refuses_a_service_that_opts_in_without_saying_what_to_route() {
        let json = r#"{
          "services": {
            "engine": { "labels": { "traefik.enable": "true" } }
          }
        }"#;

        let error = format!("{:#}", domains_in_project(&project(json)).unwrap_err());

        assert!(error.contains("engine"), "{error}");
        assert!(error.contains("no router"), "{error}");
    }

    #[test]
    /// Supports the ordinary single-domain application route.
    fn reads_the_one_host_a_simple_rule_claims() {
        assert_eq!(
            hostnames_in_rule("Host(`app.example.com`)").unwrap(),
            ["app.example.com"]
        );
    }

    #[test]
    /// Includes aliases in domain conflict checks.
    fn reads_both_sides_of_an_alternation() {
        assert_eq!(
            hostnames_in_rule("Host(`app.example.com`) || Host(`www.app.example.com`)").unwrap(),
            ["app.example.com", "www.app.example.com"]
        );
    }

    #[test]
    /// Rejects routes that Traefik cannot load.
    fn refuses_a_host_matcher_given_more_than_one_parameter() {
        let error = format!(
            "{:#}",
            hostnames_in_rule("Host(`api.app.example.com`, `admin.app.example.com`)").unwrap_err()
        );

        assert!(error.contains("one host"), "{error}");
    }

    #[test]
    /// Keeps path restrictions from changing domain ownership.
    fn ignores_matchers_that_are_not_about_the_host() {
        assert_eq!(
            hostnames_in_rule("Host(`app.example.com`) && PathPrefix(`/shop-api`)").unwrap(),
            ["app.example.com"]
        );
    }

    #[test]
    /// Refuses routing ownership that cannot be checked for conflicts.
    fn refuses_a_pattern_whose_hosts_cannot_be_listed() {
        let error = format!(
            "{:#}",
            hostnames_in_rule("HostRegexp(`^.+\\\\.example\\\\.com$`)").unwrap_err()
        );

        assert!(error.contains("HostRegexp"), "{error}");
    }

    #[test]
    /// Prevents unchecked catch-all application routes.
    fn refuses_a_rule_that_claims_no_host_at_all() {
        let error = format!("{:#}", hostnames_in_rule("PathPrefix(`/api`)").unwrap_err());

        assert!(error.contains("no host"), "{error}");
    }

    #[test]
    /// Refuses non-HTTP matchers rather than treating them as domain restrictions.
    fn refuses_non_http_matchers() {
        assert!(hostnames_in_rule("HostSNI(`*`) || Host(`app.example.com`)").is_err());
    }

    #[test]
    /// Blocks routes with an alternative that can match an undeclared hostname.
    fn refuses_unbounded_alternatives() {
        for rule in [
            "Host(`app.example.com`) || PathPrefix(`/`)",
            "PathPrefix(`/`) || Host(`app.example.com`)",
            "(Host(`app.example.com`) && Path(`/`)) || Method(`GET`)",
            "Host(`app.example.com`) && (Path(`/`) || Method(`GET`)) || Path(`/public`)",
        ] {
            assert!(hostnames_in_rule(rule).is_err(), "accepted {rule}");
        }
    }

    #[test]
    /// Prevents a negated hostname from claiming ownership of its complement.
    fn refuses_negated_hosts() {
        for rule in [
            "!Host(`app.example.com`)",
            "!(Host(`app.example.com`))",
            "Host(`app.example.com`) || !Host(`other.example.com`)",
        ] {
            assert!(hostnames_in_rule(rule).is_err(), "accepted {rule}");
        }
    }

    #[test]
    /// Keeps quoted matcher-looking text from creating a false hostname restriction.
    fn refuses_host_text_inside_an_argument() {
        assert!(hostnames_in_rule(r#"Path("/Host(`app.example.com`)")"#).is_err());
    }

    #[test]
    /// Prevents configuration that Traefik cannot parse from passing validation.
    fn refuses_incomplete_expressions() {
        for rule in [
            "Host(`app.example.com`) ||",
            "(Host(`app.example.com`)",
            "Host(`app.example.com`))",
            "Host(`app.example.com`) Path(`/`)",
            "Host(app.example.com)",
        ] {
            assert!(hostnames_in_rule(rule).is_err(), "accepted {rule}");
        }
    }

    #[test]
    /// Preserves finite domain ownership through grouping and quoted matcher arguments.
    fn supports_quoted_hosts_and_nested_guards() {
        assert_eq!(
            hostnames_in_rule(
                r#"(Host("app.example.com") || Host("www.app.example.com")) && (Path(`/Host(fake.example.com) || )`) || Method(`GET`))"#
            ).unwrap(),
            ["app.example.com", "www.app.example.com"]
        );
    }

    #[test]
    /// Accepts formatting changes without changing domain ownership.
    fn tolerates_whitespace_around_a_hostname() {
        assert_eq!(
            hostnames_in_rule("Host( `a.example.com` )").unwrap(),
            ["a.example.com"]
        );
    }
}
