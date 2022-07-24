use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::Result;
use nix::unistd::Uid;
use tokio::fs::remove_file;
use tracing::{debug, warn};
use zbus::Guid;

use crate::peer::Peer;

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Vec<Peer>,
    listener: tokio::net::UnixListener,
    guid: Guid,
    socket_path: PathBuf,
}

impl Bus {
    pub async fn new(socket_path: Option<&Path>) -> Result<Self> {
        let runtime_dir = socket_path
            .map(Path::to_path_buf)
            .or_else(|| {
                env::var("XDG_RUNTIME_DIR")
                    .ok()
                    .map(|p| Path::new(&p).to_path_buf())
            })
            .unwrap_or_else(|| {
                Path::new("/run")
                    .join("user")
                    .join(format!("{}", Uid::current()))
            });
        let socket_path = runtime_dir.join("zbusd-session");

        Ok(Self {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            peers: vec![],
            guid: Guid::generate(),
            socket_path,
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

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        remove_file(&self.socket_path).await.map_err(Into::into)
    }
}
