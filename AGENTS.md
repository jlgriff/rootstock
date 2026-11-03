# Project principles

- Design every feature for public, open-source use. Keep code, tests, examples, and documentation
  independent of any contributor's applications, infrastructure, accounts, or identity.
- Put real application inventories, domains, hosts, image references, stack settings, and secrets
  in gitignored configuration. Commit only generic templates with reserved example domains.
- Scan new and changed files for identifying information before handing work back. Do not rewrite
  Git history or remove third-party attribution without explicit authorization.
- Lead the README with the benefit and a short, usable quick start. Keep the complete configuration
  reference in linked documentation and update it whenever settings or behavior change.
- Minimize shared and per-application memory use. Keep management commands transient, build outside
  the serving VM, and measure resource usage before adding services or assuming capacity.
