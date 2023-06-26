mod cookies;

use anyhow::{bail, Error, Ok, Result};
use futures_util::{
    future::{select, Either},
    pin_mut, try_join, TryFutureExt,
};
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::{cell::OnceCell, sync::Arc};
#[cfg(unix)]
use tokio::fs::remove_file;
use tokio::{spawn, task::JoinHandle};
use tracing::{debug, info, trace, warn};
use zbus::{Address, AuthMechanism, Connection, ConnectionBuilder, Guid, Socket, TcpAddress};

use crate::peers::Peers;

/// The bus.
#[derive(Debug)]
pub struct Bus {
    inner: Inner,
    listener: Listener,
    cookie_task: Option<JoinHandle<Error>>,
}

// All (cheaply) cloneable fields of `Bus` go here.
#[derive(Clone, Debug)]
pub struct Inner {
    address: Address,
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
    pub async fn for_address(address: &str, auth_mechanism: AuthMechanism) -> Result<Self> {
        let address = address.try_into()?;

        let (listener, address) = match &address {
            #[cfg(unix)]
            Address::Unix(path) => {
                let path = Path::new(&path);
                info!("Listening on {}.", path.display());

                Self::unix_stream(path).await.map(|l| (l, address))
            }
            #[cfg(unix)]
            Address::UnixDir(dir) => {
                let dir = Path::new(&dir);
                let mut n = 0;
                loop {
                    n += 1;
                    let path = dir.join(format!("dbus-{}", n));

                    let Result::Ok(l) = Self::unix_stream(&path).await else {
                        continue;
                    };
                    info!("Listening on {}.", path.display());
                    break Ok((l, Address::Unix(path.into())));
                }
            }
            #[cfg(unix)]
            Address::UnixTmpDir(dir) => {
                let mut s = std::ffi::OsString::from("\0");
                s.push(dir);
                let dir = Path::new(&s);
                let mut n = 0;
                loop {
                    n += 1;
                    let path = dir.join(format!("dbus-{}", n));

                    let Result::Ok(l) = Self::unix_stream(&path).await else {
                        continue;
                    };
                    info!("Listening on abstract {}.", path.display());
                    break Ok((l, Address::Unix(path.into())));
                }
            }
            Address::Tcp(tcp) => {
                info!("Listening on `{}:{}`.", tcp.host(), tcp.port());

                Self::tcp_stream(tcp).await.map(|l| (l, address))
            }
            Address::NonceTcp { .. } => bail!("`nonce-tcp` transport is not supported (yet)."),
            Address::Autolaunch(_) => bail!("`autolaunch` transport is not supported (yet)."),
            _ => bail!("Unsupported address `{}`.", address),
        }?;

        let mut bus = Self::new(address.clone(), listener, auth_mechanism).await?;

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

    pub fn address(&self) -> &Address {
        &self.inner.address
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

    async fn new(
        address: Address,
        listener: Listener,
        auth_mechanism: AuthMechanism,
    ) -> Result<Self> {
        let cookie_task = if auth_mechanism == AuthMechanism::Cookie {
            let (cookie_task, cookie_sync_rx) = cookies::run_sync();
            cookie_sync_rx.await?;

            Some(cookie_task)
        } else {
            None
        };
        Ok(Self {
            listener,
            cookie_task,
            inner: Inner {
                address,
                peers: Peers::new(),
                guid: Arc::new(Guid::generate()),
                next_id: None,
                auth_mechanism,
                self_conn: OnceCell::new(),
            },
        })
    }

    #[cfg(unix)]
    async fn unix_stream(socket_path: &Path) -> Result<Listener> {
        let socket_path = socket_path.to_path_buf();
        let _ = remove_file(&socket_path).await;
        let listener = Listener::Unix {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            socket_path,
        };

        Ok(listener)
    }

    async fn tcp_stream(address: &TcpAddress) -> Result<Listener> {
        let address = (address.host(), address.port());
        let listener = Listener::Tcp {
            listener: tokio::net::TcpListener::bind(address).await?,
        };

        Ok(listener)
    }

    async fn accept_next(&mut self) -> Result<()> {
        let task = self.cookie_task.take();

        let (socket, task) = match task {
            Some(task) => {
                let accept_fut = self.accept();
                pin_mut!(accept_fut);

                match select(accept_fut, task).await {
                    Either::Left((socket, task)) => (socket?, Some(task)),
                    Either::Right((e, _)) => return Err(e?),
                }
            }
            None => (self.accept().await?, None),
        };
        self.cookie_task = task;

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
        match &mut self.listener {
            #[cfg(unix)]
            Listener::Unix {
                listener,
                socket_path,
            } => {
                let (unix_stream, _) = listener.accept().await?;
                debug!(
                    "Accepted connection on socket file {}",
                    socket_path.display()
                );

                Ok(Box::new(unix_stream))
            }
            Listener::Tcp { listener } => {
                let (tcp_stream, addr) = listener.accept().await?;
                debug!("Accepted connection from {addr}");

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
