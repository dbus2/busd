mod bus;
mod peer;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter, FmtSubscriber};

/// A simple D-Bus broker.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The socket path.
    #[clap(short = 's', long, value_parser)]
    socket_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish()
        .init();

    let args = Args::parse();

    let mut bus = bus::Bus::new(args.socket_path.as_deref()).await?;
    bus.run().await?;

    Ok(())
}
