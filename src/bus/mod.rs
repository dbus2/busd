mod cookies;

use anyhow::{bail, Ok, Result};
use clap::__macro_refs::once_cell::sync::OnceCell;
use futures_util::{try_join, TryFutureExt};
#[cfg(unix)]
use std::{
    env,
    path::{Path, PathBuf},
};
use std::{str::FromStr, sync::Arc};
#[cfg(unix)]
use tokio::fs::remove_file;
use tokio::spawn;
use tracing::{debug, info, trace, warn};
use zbus::{Address, AuthMechanism, Connection, ConnectionBuilder, Guid, Socket, TcpAddress};

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
    next_id: Option<usize>,
    auth_mechanism: AuthMechanism,
    self_conn: OnceCell<Connection>,
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
        let mut bus = match &address {
            #[cfg(unix)]
            Address::Unix(path) => {
                let path = Path::new(&path);
                info!("Listening on {}.", path.display());

                Self::unix_stream(path, auth_mechanism).await
            }
            Address::Tcp(address) => {
                info!("Listening on `{}:{}`.", address.host(), address.port());

                Self::tcp_stream(address, auth_mechanism).await
            }
            Address::NonceTcp { .. } => bail!("`nonce-tcp` transport is not supported (yet)."),
            Address::Autolaunch(_) => bail!("`autolaunch` transport is not supported (yet)."),
            _ => bail!("Unsupported address `{}`.", address),
        }?;

        // Create a peer for ourselves.
        trace!("Creating self-dial connection.");
        let conn_builder_fut = ConnectionBuilder::address(address)?
            .auth_mechanisms(&[auth_mechanism])
            .build()
            .map_err(Into::into);

        let (conn, ()) = try_join!(conn_builder_fut, bus.accept_next())?;
        bus.inner.self_conn.set(conn).unwrap();
        trace!("Self-dial connection created.");

        Ok(bus)
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            self.accept_next().await?;
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
                next_id: None,
                auth_mechanism,
                self_conn: OnceCell::new(),
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

    async fn accept_next(&mut self) -> Result<()> {
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

        Ok(())
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

    fn next_id(&mut self) -> Option<usize> {
        match self.inner.next_id {
            None => {
                self.inner.next_id = Some(0);

                None
            }
            Some(id) => {
                self.inner.next_id = Some(id + 1);

                Some(id)
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
    let path = runtime_dir.join("busd-session");

    format!("unix:path={}", path.display())
}

#[cfg(not(unix))]
fn default_address() -> String {
    "tcp:host=127.0.0.1,port=4242".to_string()
}
