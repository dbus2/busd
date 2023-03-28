use anyhow::{anyhow, Result};
use std::str::FromStr;
#[cfg(unix)]
use std::{
    env,
    path::{Path, PathBuf},
};
#[cfg(unix)]
use tokio::fs::remove_file;
use tracing::{debug, info, warn};
use zbus::{Address, AuthMechanism, Guid, Socket, TcpAddress};

use crate::{name_registry::NameRegistry, peer::Peer, peers::Peers};

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Peers,
    listener: Listener,
    guid: Guid,
    next_id: usize,
    name_registry: NameRegistry,
    auth_mechanism: AuthMechanism,
}

#[derive(Debug)]
enum Listener {
    #[cfg(unix)]
    Unix {
        listener: tokio::net::UnixListener,
        socket_path: PathBuf,
    },
    Tcp {
        listener: tokio::net::TcpListener,
    },
}

impl Bus {
    pub async fn for_address(address: Option<&str>, auth_mechanism: AuthMechanism) -> Result<Self> {
        let address = match address {
            Some(address) => address.to_string(),
            None => default_address(),
        };
        match Address::from_str(&address)? {
            #[cfg(unix)]
            Address::Unix(path) => {
                let path = Path::new(&path);
                info!("Listening on {}.", path.display());

                Self::unix_stream(path, auth_mechanism).await
            }
            #[cfg(not(unix))]
            Address::Unix(path) => {
                Err(anyhow!("`unix` transport on non-UNIX OS is not supported."))?
            }
            Address::Tcp(address) => {
                info!("Listening on `{}:{}`.", address.host(), address.port());

                Self::tcp_stream(&address, auth_mechanism).await
            }
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
            match Peer::new(
                &self.guid,
                self.next_id,
                socket,
                self.name_registry.clone(),
                self.auth_mechanism,
            )
            .await
            {
                Ok(peer) => self.peers.add(peer).await,
                Err(e) => warn!("Failed to establish connection: {}", e),
            }
            self.next_id += 1;
        }
    }

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        match self.listener {
            #[cfg(unix)]
            Listener::Unix { socket_path, .. } => {
                remove_file(socket_path).await.map_err(Into::into)
            }
            Listener::Tcp { .. } => Ok(()),
        }
    }

    fn new(listener: Listener, auth_mechanism: AuthMechanism) -> Self {
        let name_registry = NameRegistry::default();

        Self {
            listener,
            peers: Peers::new(name_registry.clone()),
            guid: Guid::generate(),
            next_id: 0,
            name_registry,
            auth_mechanism,
        }
    }

    #[cfg(unix)]
    async fn unix_stream(socket_path: &Path, auth_mechanism: AuthMechanism) -> Result<Self> {
        let socket_path = socket_path.to_path_buf();
        let listener = Listener::Unix {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            socket_path,
        };

        Ok(Self::new(listener, auth_mechanism))
    }

    async fn tcp_stream(address: &TcpAddress, auth_mechanism: AuthMechanism) -> Result<Self> {
        let address = (address.host(), address.port());
        let listener = Listener::Tcp {
            listener: tokio::net::TcpListener::bind(address).await?,
        };

        Ok(Self::new(listener, auth_mechanism))
    }

    async fn accept(&mut self) -> Result<Box<dyn Socket + 'static>> {
        match &mut self.listener {
            #[cfg(unix)]
            Listener::Unix {
                listener,
                socket_path: _,
            } => {
                let (unix_stream, addr) = listener.accept().await?;
                debug!("Accepted connection from {:?}", addr);

                Ok(Box::new(unix_stream))
            }
            Listener::Tcp { listener } => {
                let (tcp_stream, addr) = listener.accept().await?;
                debug!("Accepted connection from {:?}", addr);

                Ok(Box::new(tcp_stream))
            }
        }
    }
}

#[cfg(unix)]
fn default_address() -> String {
    let runtime_dir = env::var("XDG_RUNTIME_DIR")
        .as_ref()
        .map(|s| Path::new(s).to_path_buf())
        .ok()
        .unwrap_or_else(|| {
            Path::new("/run")
                .join("user")
                .join(format!("{}", nix::unistd::Uid::current()))
        });
    let path = runtime_dir.join("dbuz-session");

    format!("unix:path={}", path.display())
}

#[cfg(not(unix))]
fn default_address() -> String {
    "tcp:host=127.0.0.1,port=4242".to_string()
}
