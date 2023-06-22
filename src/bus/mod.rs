mod cookies;

use anyhow::{anyhow, Result};
#[cfg(unix)]
use std::{
    env,
    path::{Path, PathBuf},
};
use std::{str::FromStr, sync::Arc};
use tokio::{fs::remove_file, spawn};
use tracing::{debug, info, warn};
use zbus::{Address, AuthMechanism, Guid, Socket, TcpAddress};

use crate::peers::Peers;

/// The bus.
#[derive(Debug)]
pub struct Bus {
    inner: Inner,
    listener: Listener,
}

// All (cheaply) cloneable fields of `Bus` go here.
#[derive(Clone, Debug)]
pub struct Inner {
    peers: Arc<Peers>,
    guid: Arc<Guid>,
    next_id: usize,
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
        match &address {
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
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            let socket = self.accept().await?;

            let id = self.next_id();
            let inner = self.inner.clone();
            spawn(async move {
                if let Err(e) = inner
                    .peers
                    .clone()
                    .add(&inner.guid, id, socket, inner.auth_mechanism)
                    .await
                {
                    warn!("Failed to establish connection: {}", e);
                }
            });
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
        Self {
            listener,
            inner: Inner {
                peers: Peers::new(),
                guid: Arc::new(Guid::generate()),
                next_id: 0,
                auth_mechanism,
            },
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
        if self.auth_mechanism() == AuthMechanism::Cookie {
            cookies::sync().await?;
        }
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
        &self.inner.peers
    }

    pub fn guid(&self) -> &Arc<Guid> {
        &self.inner.guid
    }

    pub fn auth_mechanism(&self) -> AuthMechanism {
        self.inner.auth_mechanism
    }

    fn next_id(&mut self) -> usize {
        self.inner.next_id += 1;

        self.inner.next_id
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
