use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use nix::unistd::Uid;
use tokio::fs::remove_file;
use tracing::{debug, warn};
use zbus::{names::OwnedUniqueName, Guid};

use crate::peer::Peer;

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: BTreeMap<OwnedUniqueName, Peer>,
    listener: tokio::net::UnixListener,
    guid: Guid,
    socket_path: PathBuf,
    next_id: usize,
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
            peers: BTreeMap::new(),
            guid: Guid::generate(),
            socket_path,
            next_id: 0,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        while let Ok((unix_stream, addr)) = self.listener.accept().await {
            debug!("Accepted connection from {:?}", addr);
            if let Err(e) = Peer::new(&self.guid, self.next_id, unix_stream)
                .await
                .and_then(|peer| {
                    let unique_name = peer.unique_name().clone();
                    match self.peers.insert(unique_name, peer) {
                        Some(peer) => {
                            Err(anyhow!("Unique name `{}` already used", peer.unique_name()))
                        }
                        None => Ok(()),
                    }
                })
            {
                warn!("Failed to establish connection: {}", e);
            }
            self.next_id += 1;
        }

        Ok(())
    }

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        remove_file(&self.socket_path).await.map_err(Into::into)
    }
}
