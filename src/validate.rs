use std::collections::HashMap;

use crate::inventory::App;

/// Every reason the given set of applications cannot be served together.
pub fn conflicts(apps: &[App]) -> Vec<String> {
    let mut found = duplicate_names(apps);
    found.extend(duplicate_domains(apps));
    found
}

/// Reports identifiers shared by several applications, which makes a deployment ambiguous.
fn duplicate_names(apps: &[App]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for app in apps {
        *counts.entry(app.discovered.name.as_str()).or_default() += 1;
    }

    let mut reported: Vec<String> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(name, count)| format!("{count} applications share the name `{name}`"))
        .collect();
    reported.sort();
    reported
}

/// Reports hostnames claimed more than once, which Traefik would resolve unpredictably.
fn duplicate_domains(apps: &[App]) -> Vec<String> {
    let mut owner: HashMap<String, &str> = HashMap::new();
    let mut reported = Vec::new();

    for app in apps {
        let name = app.discovered.name.as_str();
        for domain in &app.domains {
            let domain = domain.to_lowercase();
            match owner.get(domain.as_str()) {
                Some(first) if *first == name => {
                    reported.push(format!("domain `{domain}` is claimed twice by `{name}`"))
                }
                Some(first) => reported.push(format!(
                    "domain `{domain}` is claimed by both `{first}` and `{name}`"
                )),
                None => {
                    owner.insert(domain, name);
                }
            }
        }
    }
    reported
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Discovered, PROJECT_PREFIX};
    use std::path::PathBuf;

    /// Builds an isolated application definition for the behavior under test.
    fn app(name: &str, domains: &[&str]) -> App {
        App {
            discovered: Discovered {
                name: name.to_string(),
                directory: PathBuf::from(format!("/srv/rootstock/apps/{name}")),
                project: format!("{PROJECT_PREFIX}{name}"),
                profiles: Vec::new(),
            },
            domains: domains.iter().map(|d| d.to_string()).collect(),
        }
    }

    #[test]
    /// Allows unrelated applications to coexist on one proxy.
    fn accepts_applications_claiming_different_domains() {
        let apps = [
            app("web", &["app.example.com"]),
            app("worker", &["worker.example.com"]),
        ];

        assert_eq!(conflicts(&apps), Vec::<String>::new());
    }

    #[test]
    /// Prevents ambiguous routing between applications.
    fn reports_a_domain_claimed_by_two_applications() {
        let apps = [
            app("web", &["app.example.com"]),
            app("other", &["app.example.com"]),
        ];

        let found = conflicts(&apps);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("app.example.com"), "{}", found[0]);
        assert!(found[0].contains("other"), "{}", found[0]);
    }

    #[test]
    /// Prevents hostname casing from bypassing conflict detection.
    fn treats_domains_as_case_insensitive() {
        let apps = [
            app("web", &["app.example.com"]),
            app("other", &["App.Example.com"]),
        ];

        assert_eq!(conflicts(&apps).len(), 1);
    }

    #[test]
    /// Prevents ambiguous duplicate routes within an application.
    fn reports_a_domain_repeated_inside_one_application() {
        let apps = [app("web", &["app.example.com", "app.example.com"])];

        assert_eq!(conflicts(&apps).len(), 1);
    }

    #[test]
    /// Prevents deployments from selecting an ambiguous application.
    fn reports_two_applications_sharing_a_name() {
        let apps = [
            app("web", &["app.example.com"]),
            app("web", &["other.example.com"]),
        ];

        let found = conflicts(&apps);

        let named: Vec<_> = found
            .iter()
            .filter(|m| m.contains("share the name"))
            .collect();
        assert_eq!(named.len(), 1, "{found:?}");
    }

    #[test]
    /// Lets operators fix routing conflicts in one pass.
    fn reports_every_problem_rather_than_only_the_first() {
        let apps = [
            app("web", &["app.example.com", "shop.example.com"]),
            app("rival", &["app.example.com", "shop.example.com"]),
        ];

        assert_eq!(conflicts(&apps).len(), 2);
    }
}
