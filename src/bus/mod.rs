mod cookies;

use anyhow::{anyhow, Result};
#[cfg(unix)]
use std::{
    env,
    path::{Path, PathBuf},
};
use std::{str::FromStr, sync::Arc};
use tokio::{fs::remove_file, spawn, sync::mpsc::Receiver};
use tracing::{debug, info, warn};
use zbus::{Address, AuthMechanism, Guid, Socket, TcpAddress};

use crate::{
    name_registry::NameOwnerChanged, peer::Peer, peers::Peers, self_dial_conn::SelfDialConn,
};

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Arc<Peers>,
    listener: Listener,
    guid: Arc<Guid>,
    next_id: Option<usize>,
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
        let address = Address::from_str(&address)?;
        let (mut bus, name_changed_rx) = match &address {
            #[cfg(unix)]
            Address::Unix(path) => {
                let path = Path::new(&path);
                info!("Listening on {}.", path.display());

                Self::unix_stream(path, auth_mechanism).await
            }
            #[cfg(not(unix))]
            Address::Unix(_) => Err(anyhow!("`unix` transport on non-UNIX OS is not supported."))?,
            Address::Tcp(address) => {
                info!("Listening on `{}:{}`.", address.host(), address.port());

                Self::tcp_stream(address, auth_mechanism).await
            }
            Address::NonceTcp { .. } => {
                Err(anyhow!("`nonce-tcp` transport is not supported (yet)."))?
            }
            Address::Autolaunch(_) => {
                Err(anyhow!("`autolaunch` transport is not supported (yet)."))?
            }
            _ => Err(anyhow!("Unsupported address `{}`.", address))?,
        }?;

        // Create a peer for ourselves.
        let (self_dial_conn, self_dial_peer) =
            SelfDialConn::connect(&mut bus, address.clone()).await?;
        bus.peers.add(self_dial_peer).await;
        spawn(self_dial_conn.monitor_name_changes(name_changed_rx));

        Ok(bus)
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.accept_next().await {
                Ok(peer) => self.peers.add(peer).await,
                Err(e) => warn!("Failed to establish connection: {}", e),
            }
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

    fn new(
        listener: Listener,
        auth_mechanism: AuthMechanism,
    ) -> (Self, Receiver<NameOwnerChanged>) {
        let (peers, name_changed_rx) = Peers::new();
        (
            Self {
                listener,
                peers: Arc::new(peers),
                guid: Arc::new(Guid::generate()),
                next_id: None,
                auth_mechanism,
            },
            name_changed_rx,
        )
    }

    #[cfg(unix)]
    async fn unix_stream(
        socket_path: &Path,
        auth_mechanism: AuthMechanism,
    ) -> Result<(Self, Receiver<NameOwnerChanged>)> {
        let socket_path = socket_path.to_path_buf();
        let listener = Listener::Unix {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            socket_path,
        };

        Ok(Self::new(listener, auth_mechanism))
    }

    async fn tcp_stream(
        address: &TcpAddress,
        auth_mechanism: AuthMechanism,
    ) -> Result<(Self, Receiver<NameOwnerChanged>)> {
        let address = (address.host(), address.port());
        let listener = Listener::Tcp {
            listener: tokio::net::TcpListener::bind(address).await?,
        };

        Ok(Self::new(listener, auth_mechanism))
    }

    pub async fn accept_next(&mut self) -> Result<Peer> {
        let socket = self.accept().await?;
        if self.auth_mechanism == AuthMechanism::Cookie {
            cookies::sync().await?;
        }
        Peer::new(
            &self.guid,
            self.next_id,
            socket,
            self.auth_mechanism,
            self.peers.clone(),
        )
        .await
        .map(|peer| {
            match self.next_id.as_mut() {
                Some(id) => *id += 1,
                None => self.next_id = Some(0),
            }

            peer
        })
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

    pub fn peers(&self) -> &Arc<Peers> {
        &self.peers
    }

    pub fn guid(&self) -> &Arc<Guid> {
        &self.guid
    }

    pub fn auth_mechanism(&self) -> AuthMechanism {
        self.auth_mechanism
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
    let path = runtime_dir.join("busd-session");

    format!("unix:path={}", path.display())
}

#[cfg(not(unix))]
fn default_address() -> String {
    "tcp:host=127.0.0.1,port=4242".to_string()
}
