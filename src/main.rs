use std::path::PathBuf;
use std::process::Command as Process;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rootstock::inventory::App;
use rootstock::{dashboard, deployment, inventory, status, validate};

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
    /// Reports live containers, domains, ports, health, and resource usage.
    Status {
        /// Print the snapshot as JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Serves an on-demand status page on local loopback.
    Observe {
        /// Local HTTP port; zero chooses an available port.
        #[arg(long, default_value_t = 8787)]
        port: u16,
        /// Seconds between browser refreshes.
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
        refresh: u64,
    },
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

/// Runs inventory, deployment, or live observation with the required application settings.
fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::List => list(&inventory::load(&cli.apps)?),
        Command::Check => {
            let apps = inventory::load(&cli.apps)?;
            ensure_no_conflicts(&apps)?;
            println!("{} application(s), no conflicts", apps.len());
        }
        Command::Deploy { name, dry_run } => {
            let apps = inventory::load(&cli.apps)?;
            let plan = deployment::plan(&apps, &name)?;
            run(&plan.docker_args, dry_run)?;
        }
        Command::Status { json } => {
            let snapshot = status::collect(&cli.apps)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                print!("{}", status::table(&snapshot));
            }
        }
        Command::Observe { port, refresh } => dashboard::serve(&cli.apps, port, refresh)?,
    }
    Ok(())
}
