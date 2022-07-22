mod peer;

use std::env;

use clap::Parser;
use nix::unistd::Uid;
use tracing::{debug, error};
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter, FmtSubscriber};
use zbus::Guid;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The socket path.
    #[clap(short = 's', long, value_parser)]
    socket_path: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish()
        .init();

    let args = Args::parse();

    let runtime_dir = args
        .socket_path
        .or_else(|| env::var("XDG_RUNTIME_DIR").ok())
        .unwrap_or_else(|| format!("/run/user/{}", Uid::current()));
    let path = format!("{}/zbusd-session", runtime_dir);
    let listener = tokio::net::UnixListener::bind(&path)?;
    let mut peers = vec![];
    let guid = Guid::generate();

    while let Ok((unix_stream, addr)) = listener.accept().await {
        debug!("Accepted connection from {:?}", addr);
        match peer::Peer::new(&guid, unix_stream).await {
            Ok(peer) => peers.push(peer),
            Err(e) => error!("{}", e),
        }
    }

    Ok(())
}