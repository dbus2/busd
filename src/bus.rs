use anyhow::Result;
use nix::unistd::Uid;
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::fs::remove_file;
use tracing::{debug, warn};
use zbus::Guid;

use crate::{name_registry::NameRegistry, peer::Peer, peers::Peers};

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Peers,
    listener: tokio::net::UnixListener,
    guid: Guid,
    socket_path: PathBuf,
    next_id: usize,
    name_registry: NameRegistry,
}

impl Bus {
    pub async fn new(socket_path: Option<&Path>) -> Result<Self> {
        let socket_path = socket_path
            .map(Path::to_path_buf)
            .or_else(|| {
                env::var("XDG_RUNTIME_DIR")
                    .ok()
                    .map(|p| Path::new(&p).to_path_buf().join("dbuz-session"))
            })
            .unwrap_or_else(|| {
                Path::new("/run")
                    .join("user")
                    .join(format!("{}", Uid::current()))
                    .join("dbuz-session")
            });

        Ok(Self {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            peers: Peers::new(),
            guid: Guid::generate(),
            socket_path,
            next_id: 0,
            name_registry: NameRegistry::default(),
        })
    }

    pub async fn run(&mut self) {
        while let Ok((unix_stream, addr)) = self.listener.accept().await {
            debug!("Accepted connection from {:?}", addr);
            match Peer::new(
                &self.guid,
                self.next_id,
                unix_stream,
                self.name_registry.clone(),
            )
            .await
            {
                Ok(peer) => self.peers.add(peer),
                Err(e) => warn!("Failed to establish connection: {}", e),
            }
            self.next_id += 1;
        }
    }

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        remove_file(&self.socket_path).await.map_err(Into::into)
    }
}
