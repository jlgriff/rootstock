use std::fs;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::HostMemory;

/// Reads Linux host counters without installing an agent or executing inside app containers.
pub(super) fn collect(endpoint: &str) -> Result<HostMemory> {
    let contents = if let Some(path) = std::env::var_os("ROOTSTOCK_HOST_MEMINFO") {
        fs::read_to_string(path).context("reading mounted host memory counters")?
    } else if endpoint.starts_with("ssh://") {
        let output = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                "--",
                endpoint,
                "cat /proc/meminfo",
            ])
            .output()
            .context("reading host memory over SSH")?;
        if !output.status.success() {
            bail!(
                "reading host memory over SSH: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("reading Linux memory counters")?
    } else if cfg!(target_os = "linux") && endpoint.starts_with("unix://") {
        fs::read_to_string("/proc/meminfo").context("reading local Linux memory counters")?
    } else {
        bail!("Connect to the Linux Docker host over SSH to read its memory");
    };
    parse(&contents)
}

/// Converts kernel counters to bytes while keeping available memory and cache distinct.
fn parse(contents: &str) -> Result<HostMemory> {
    let counter = |name: &str| -> Result<u64> {
        let value = contents
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .with_context(|| format!("missing {name} in /proc/meminfo"))?
            .trim()
            .strip_suffix("kB")
            .with_context(|| format!("invalid {name} unit in /proc/meminfo"))?
            .trim()
            .parse::<u64>()
            .with_context(|| format!("invalid {name} in /proc/meminfo"))?;
        value.checked_mul(1024).context("memory counter overflow")
    };
    let total_bytes = counter("MemTotal")?;
    let available_bytes = counter("MemAvailable")?;
    let swap_total_bytes = counter("SwapTotal")?;
    Ok(HostMemory {
        total_bytes,
        used_bytes: total_bytes
            .checked_sub(available_bytes)
            .context("available memory exceeds total")?,
        available_bytes,
        cache_and_buffers_bytes: counter("Cached")?
            + counter("Buffers")?
            + counter("SReclaimable")?,
        swap_total_bytes,
        swap_used_bytes: swap_total_bytes
            .checked_sub(counter("SwapFree")?)
            .context("free swap exceeds total")?,
    })
}
