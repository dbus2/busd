mod cookies;

use anyhow::{bail, Error, Ok, Result};
use futures_util::{
    future::{select, Either},
    pin_mut, try_join, TryFutureExt,
};
use std::{cell::OnceCell, str::FromStr, sync::Arc};
#[cfg(unix)]
use std::{env, path::Path};
#[cfg(unix)]
use tokio::fs::remove_file;
use tokio::{spawn, task::JoinHandle};
use tracing::{debug, info, trace, warn};
#[cfg(unix)]
use zbus::address::transport::{Unix, UnixSocket};
use zbus::{
    address::{transport::Tcp, Transport},
    connection::socket::BoxedSplit,
    Address, AuthMechanism, Connection, ConnectionBuilder, Guid, OwnedGuid,
};

use crate::{
    fdo::{self, DBus, Monitoring},
    peers::Peers,
};

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
    guid: OwnedGuid,
    next_id: Option<usize>,
    auth_mechanism: AuthMechanism,
    self_conn: OnceCell<Connection>,
}

#[derive(Debug)]
enum Listener {
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),
    Tcp(tokio::net::TcpListener),
}

impl Bus {
    pub async fn for_address(address: Option<&str>, auth_mechanism: AuthMechanism) -> Result<Self> {
        let mut address = match address {
            Some(address) => Address::from_str(address)?,
            None => Address::from_str(&default_address())?,
        };
        let guid: OwnedGuid = match address.guid() {
            Some(guid) => guid.to_owned().into(),
            None => {
                let guid = Guid::generate();
                address = address.set_guid(guid.clone())?;

                guid.into()
            }
        };
        let listener = match address.transport() {
            #[cfg(unix)]
            Transport::Unix(unix) => Self::unix_stream(unix).await,
            Transport::Tcp(tcp) => Self::tcp_stream(tcp).await,
            #[cfg(windows)]
            Transport::Autolaunch(_) => bail!("`autolaunch` transport is not supported (yet)."),
            _ => bail!("Unsupported address `{}`.", address),
        }?;

        let mut bus = Self::new(address.clone(), guid.clone(), listener, auth_mechanism).await?;

        // Create a peer for ourselves.
        trace!("Creating self-dial connection.");
        let dbus = DBus::new(bus.peers().clone(), guid.clone());
        let monitoring = Monitoring::new(bus.peers().clone());
        let conn_builder_fut = ConnectionBuilder::address(address)?
            .auth_mechanisms(&[auth_mechanism])
            .p2p()
            .unique_name(fdo::BUS_NAME)?
            .name(fdo::BUS_NAME)?
            .serve_at(fdo::DBus::PATH, dbus)?
            .serve_at(fdo::Monitoring::PATH, monitoring)?
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
        match self.inner.address.transport() {
            #[cfg(unix)]
            Transport::Unix(unix) => match unix.path() {
                UnixSocket::File(path) => remove_file(path).await.map_err(Into::into),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    async fn new(
        address: Address,
        guid: OwnedGuid,
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
                guid,
                next_id: None,
                auth_mechanism,
                self_conn: OnceCell::new(),
            },
        })
    }

    #[cfg(unix)]
    async fn unix_stream(unix: &Unix) -> Result<Listener> {
        // TODO: Use tokio::net::UnixListener directly once it supports abstract sockets:
        //
        // https://github.com/tokio-rs/tokio/issues/4610

        use std::os::unix::net::SocketAddr;

        let addr = match unix.path() {
            #[cfg(target_os = "linux")]
            UnixSocket::Abstract(name) => {
                use std::os::linux::net::SocketAddrExt;

                let addr = SocketAddr::from_abstract_name(name.as_encoded_bytes())?;
                info!(
                    "Listening on abstract UNIX socket `{}`.",
                    name.to_string_lossy()
                );

                addr
            }
            UnixSocket::File(path) => {
                let addr = SocketAddr::from_pathname(path)?;
                info!(
                    "Listening on UNIX socket file `{}`.",
                    path.to_string_lossy()
                );

                addr
            }
            UnixSocket::Dir(_) => bail!("`dir` transport is not supported (yet)."),
            UnixSocket::TmpDir(_) => bail!("`tmpdir` transport is not supported (yet)."),
            _ => bail!("Unsupported address."),
        };
        let std_listener =
            tokio::task::spawn_blocking(move || std::os::unix::net::UnixListener::bind_addr(&addr))
                .await??;
        std_listener.set_nonblocking(true)?;
        tokio::net::UnixListener::from_std(std_listener)
            .map(Listener::Unix)
            .map_err(Into::into)
    }

    async fn tcp_stream(tcp: &Tcp) -> Result<Listener> {
        if tcp.nonce_file().is_some() {
            bail!("`nonce-tcp` transport is not supported (yet).");
        }
        info!("Listening on `{}:{}`.", tcp.host(), tcp.port());
        let address = (tcp.host(), tcp.port());

        tokio::net::TcpListener::bind(address)
            .await
            .map(Listener::Tcp)
            .map_err(Into::into)
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

    async fn accept(&mut self) -> Result<BoxedSplit> {
        let stream = match &mut self.listener {
            #[cfg(unix)]
            Listener::Unix(listener) => listener.accept().await.map(|(stream, _)| stream.into())?,
            Listener::Tcp(listener) => listener.accept().await.map(|(stream, _)| stream.into())?,
        };
        debug!("Accepted connection on address `{}`", self.inner.address);

        Ok(stream)
    }

    pub fn peers(&self) -> &Arc<Peers> {
        &self.inner.peers
    }

    pub fn guid(&self) -> &OwnedGuid {
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
