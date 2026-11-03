use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags};

const MARKER: &str = "restore-marker";

/// Opens the prepared database without silently creating a replacement.
fn open_existing(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| format!("opening prepared database {}", path.display()))?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

/// Holds a WAL connection open so replication exercises a live application.
fn serve(path: &Path) -> Result<()> {
    let connection = open_existing(path)?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         CREATE TABLE IF NOT EXISTS checks (token TEXT NOT NULL);",
    )?;
    loop {
        std::thread::park();
    }
}

/// Confirms recovery restored an intact database containing exactly the committed marker.
fn verify(path: &Path) -> Result<()> {
    let connection = open_existing(path)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    ensure!(
        integrity == "ok",
        "database integrity check failed: {integrity}"
    );
    let mut statement = connection.prepare("SELECT token FROM checks")?;
    let tokens = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        tokens == [MARKER],
        "restored database has unexpected markers"
    );
    Ok(())
}

/// Simulates disk loss while allowing the drill to inspect restart behavior.
fn remove_database(path: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut file = path.as_os_str().to_os_string();
        file.push(suffix);
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Serves the synthetic app or runs a database assertion for the recovery drill.
fn main() -> Result<()> {
    let path = PathBuf::from(env::var_os("DATABASE_PATH").unwrap_or_else(|| "/data/app.db".into()));
    match env::args().nth(1).as_deref().unwrap_or("serve") {
        "serve" => serve(&path),
        "health" => {
            open_existing(&path)?.query_row("SELECT count(*) FROM checks", [], |row| {
                row.get::<_, i64>(0)
            })?;
            Ok(())
        }
        "insert" => {
            open_existing(&path)?.execute("INSERT INTO checks VALUES (?)", [MARKER])?;
            Ok(())
        }
        "verify" => verify(&path),
        "assert-missing" => {
            ensure!(
                !path.try_exists()?,
                "failed recovery left a database behind"
            );
            Ok(())
        }
        "remove" => remove_database(&path),
        command => bail!("unknown fixture command: {command}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Protects recovery failures from becoming an empty application database.
    #[test]
    fn missing_database_is_refused_without_creating_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        assert!(
            open_existing(&path).is_err(),
            "missing database was accepted"
        );
        assert!(!path.exists(), "opening a missing database created a file");
    }
}
