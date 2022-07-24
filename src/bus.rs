use std::env;

use anyhow::Result;
use nix::unistd::Uid;
use tracing::{debug, warn};
use zbus::Guid;

use crate::peer::Peer;

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Vec<Peer>,
    listener: tokio::net::UnixListener,
    guid: Guid,
}

impl Bus {
    pub async fn new(socket_path: Option<String>) -> Result<Self> {
        let runtime_dir = socket_path
            .or_else(|| env::var("XDG_RUNTIME_DIR").ok())
            .unwrap_or_else(|| format!("/run/user/{}", Uid::current()));
        let path = format!("{}/zbusd-session", runtime_dir);

        Ok(Self {
            listener: tokio::net::UnixListener::bind(&path)?,
            peers: vec![],
            guid: Guid::generate(),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Ok((unix_stream, addr)) = self.listener.accept().await {
            debug!("Accepted connection from {:?}", addr);
            match Peer::new(&self.guid, unix_stream).await {
                Ok(peer) => self.peers.push(peer),
                Err(e) => warn!("Failed to establish connection: {}", e),
            }
        }

        Ok(())
    }
}