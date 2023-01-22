use anyhow::Result;
use nix::unistd::Uid;
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::fs::remove_file;
use tracing::{debug, warn};
use zbus::{Guid, Socket};

use crate::{name_registry::NameRegistry, peer::Peer, peers::Peers};

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Peers,
    listener: Listener,
    guid: Guid,
    next_id: usize,
    name_registry: NameRegistry,
}

#[derive(Debug)]
enum Listener {
    Unix {
        listener: tokio::net::UnixListener,
        socket_path: PathBuf,
    },
}

impl Bus {
    pub async fn unix_stream(socket_path: Option<&Path>) -> Result<Self> {
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
        let listener = Listener::Unix {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            socket_path,
        };

        Ok(Self::new(listener))
    }

    pub async fn run(&mut self) {
        while let Ok(socket) = self.accept().await {
            match Peer::new(&self.guid, self.next_id, socket, self.name_registry.clone()).await {
                Ok(peer) => self.peers.add(peer).await,
                Err(e) => warn!("Failed to establish connection: {}", e),
            }
            self.next_id += 1;
        }
    }

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        match self.listener {
            Listener::Unix { socket_path, .. } => {
                remove_file(socket_path).await.map_err(Into::into)
            }
        }
    }

    fn new(listener: Listener) -> Self {
        let name_registry = NameRegistry::default();

        Self {
            listener,
            peers: Peers::new(name_registry.clone()),
            guid: Guid::generate(),
            next_id: 0,
            name_registry,
        }
    }

    async fn accept(&mut self) -> Result<Box<dyn Socket + 'static>> {
        match &mut self.listener {
            Listener::Unix {
                listener,
                socket_path: _,
            } => {
                let (unix_stream, addr) = listener.accept().await?;
                debug!("Accepted connection from {:?}", addr);

                Ok(Box::new(unix_stream))
            }
        }
    }
}
