use std::path::PathBuf;
use std::process::Command as Process;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rootstock::inventory::App;
use rootstock::{compose, inventory, validate};

#[derive(Parser)]
#[command(version, about = "Coordinates the applications sharing one Linux VM")]
struct Cli {
    /// Directory of application folders, each holding one Compose file.
    #[arg(long, global = true, default_value = "apps")]
    apps: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lists every application and the domains it claims.
    List,
    /// Reports routing conflicts without changing anything.
    Check,
    /// Brings one application up under its own Compose project.
    Deploy {
        /// The application to deploy.
        name: String,

        /// Show what would run without running it.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Refuses to go further while any application would collide with another.
fn ensure_no_conflicts(apps: &[App]) -> Result<()> {
    let conflicts = validate::conflicts(apps);
    if conflicts.is_empty() {
        return Ok(());
    }

    for conflict in &conflicts {
        eprintln!("conflict: {conflict}");
    }
    bail!("{} routing conflicts, nothing deployed", conflicts.len())
}

/// Finds the one application a command names, so an unknown name fails before anything runs.
fn find<'a>(apps: &'a [App], name: &str) -> Result<&'a App> {
    apps.iter()
        .find(|app| app.discovered.name == name)
        .with_context(|| format!("no application named `{name}` under the applications directory"))
}

/// Prints each application with the domains Traefik will route to it.
fn list(apps: &[App]) {
    for app in apps {
        println!("{} ({})", app.discovered.name, app.discovered.project);
        for domain in &app.domains {
            println!("  {domain}");
        }
        if app.domains.is_empty() {
            println!("  (claims no domain)");
        }
    }
}

/// Runs one Compose invocation, or describes it when the run is a rehearsal.
fn run(args: &[String], dry_run: bool) -> Result<()> {
    let shown = format!("docker {}", args.join(" "));
    if dry_run {
        println!("would run {shown}");
        return Ok(());
    }

    let status = Process::new("docker")
        .args(args)
        .status()
        .with_context(|| format!("running `{shown}`"))?;
    if !status.success() {
        bail!("`{shown}` failed");
    }

    println!("ran {shown}");
    Ok(())
}

/// Validates application ownership before dispatching the requested operation.
fn main() -> Result<()> {
    let cli = Cli::parse();
    let apps = inventory::load(&cli.apps)?;

    match cli.command {
        Command::List => list(&apps),
        Command::Check => {
            ensure_no_conflicts(&apps)?;
            println!("{} application(s), no conflicts", apps.len());
        }
        Command::Deploy { name, dry_run } => {
            ensure_no_conflicts(&apps)?;
            let app = find(&apps, &name)?;
            run(&compose::up_args(&app.discovered), dry_run)?;
        }
    }
    Ok(())
}
