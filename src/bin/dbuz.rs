extern crate dbuz;

use dbuz::bus;

use anyhow::Result;
use clap::Parser;
#[cfg(unix)]
use tokio::{select, signal::unix::SignalKind};
use tracing::{error, info, warn};

/// A simple D-Bus broker.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The address to listen on.
    #[clap(short = 'a', long, value_parser)]
    address: Option<String>,

    /// Allow anonymous connections.
    #[clap(long)]
    allow_anonymous: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dbuz::tracing_subscriber::init();

    let args = Args::parse();

    let mut bus = bus::Bus::for_address(args.address.as_deref(), args.allow_anonymous).await?;

    // FIXME: How to handle this gracefully on Windows?
    #[cfg(unix)]
    {
        let mut sig_int = tokio::signal::unix::signal(SignalKind::interrupt())?;

        select! {
            _ = sig_int.recv() => {
                info!("Received SIGINT, shutting down..");
            }
            _ = bus.run() => {
                warn!("Bus stopped, shutting down..");
            }
        }
    }
    #[cfg(not(unix))]
    bus.run().await;

    if let Err(e) = bus.cleanup().await {
        error!("Failed to clean up: {}", e);
    }

    Ok(())
}
