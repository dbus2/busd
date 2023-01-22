use anyhow::{anyhow, Result};
use nix::unistd::Uid;
use std::{
    env,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::fs::remove_file;
use tracing::{debug, info, warn};
use zbus::{Address, Guid, Socket};

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
    pub async fn for_address(address: Option<&str>) -> Result<Self> {
        let address = match address {
            Some(address) => address.to_string(),
            None => default_address(),
        };
        match Address::from_str(&address)? {
            Address::Unix(path) => {
                let path = Path::new(&path);
                info!("Listening on {}.", path.display());

                Self::unix_stream(path).await
            }
            Address::Tcp(_) => Err(anyhow!("`tcp` transport is not supported (yet)."))?,
            Address::NonceTcp { .. } => {
                Err(anyhow!("`nonce-tcp` transport is not supported (yet)."))?
            }
            Address::Autolaunch(_) => {
                Err(anyhow!("`autolaunch` transport is not supported (yet)."))?
            }
            _ => Err(anyhow!("Unsupported address `{}`.", address))?,
        }
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

    async fn unix_stream(socket_path: &Path) -> Result<Self> {
        let socket_path = socket_path.to_path_buf();
        let listener = Listener::Unix {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            socket_path,
        };

        Ok(Self::new(listener))
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

fn default_address() -> String {
    let runtime_dir = env::var("XDG_RUNTIME_DIR")
        .as_ref()
        .map(|s| Path::new(s).to_path_buf())
        .ok()
        .unwrap_or_else(|| {
            Path::new("/run")
                .join("user")
                .join(format!("{}", Uid::current()))
        });
    let path = runtime_dir.join("dbuz-session");

    format!("unix:path={}", path.display())
}
