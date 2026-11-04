use std::error::Error;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Probes HTTP readiness without requiring a shell or a separate client in the image.
fn healthcheck(mut address: SocketAddr) -> Result<(), Box<dyn Error>> {
    if address.ip().is_unspecified() {
        address.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut status = [0; 12];
    stream.read_exact(&mut status)?;
    if &status != b"HTTP/1.1 200" {
        return Err("HTTP readiness check failed".into());
    }
    Ok(())
}

/// Stops accepting requests when the container or terminal requests shutdown.
async fn shutdown() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing the termination handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate.recv() => {},
    }
}

/// Runs the server or its container readiness probe on the configured address.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = std::env::var("SITE_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into())
        .parse()?;
    match std::env::args().nth(1).as_deref() {
        Some("--healthcheck") => return healthcheck(address),
        Some(_) => return Err("usage: leptos-placeholder [--healthcheck]".into()),
        None => {}
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("http://{}", listener.local_addr()?);
    axum::serve(listener, leptos_placeholder::router())
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}
