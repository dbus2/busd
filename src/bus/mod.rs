mod cookies;

use anyhow::{bail, Error, Ok, Result};
use futures_util::{
    future::{select, Either},
    pin_mut,
};
use std::sync::Arc;
#[cfg(unix)]
use tokio::fs::remove_file;
use tokio::task::{spawn, JoinHandle};
use tracing::{debug, info, trace, warn};
#[cfg(unix)]
use zbus::address::transport::Unix;
use zbus::{
    address::transport::{Tcp, Transport},
    connection::{self, socket::BoxedSplit},
    Address, AuthMechanism, Connection, Guid, OwnedGuid,
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
    address: String,
    peers: Arc<Peers>,
    guid: OwnedGuid,
    next_id: usize,
    auth_mechanism: AuthMechanism,
    _self_conn: Connection,
}

#[derive(Debug)]
enum Listener {
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),
    Tcp(tokio::net::TcpListener),
}

impl Bus {
    pub async fn for_address(address: &str, auth_mechanism: AuthMechanism) -> Result<Self> {
        let address = Address::try_from(address)?;
        let (guid, addr) = match address.guid() {
            Some(guid) => (Guid::try_from(guid)?.to_owned().into(), address.to_string()),
            None => {
                let guid: OwnedGuid = Guid::generate().into();
                let addr = format!("{address},guid={guid}");
                (guid, addr)
            }
        };

        let listener = match &address.transport()? {
            #[cfg(unix)]
            Transport::Unix(unix) => Self::unix_stream(unix).await,
            Transport::Tcp(tcp) => Self::tcp_stream(tcp).await,
            #[cfg(windows)]
            Transport::Autolaunch(_) => bail!("`autolaunch` transport is not supported (yet)."),
            _ => bail!("Unsupported address `{}`.", address),
        }?;

        let peers = Peers::new();

        let dbus = DBus::new(peers.clone(), guid.clone());
        let monitoring = Monitoring::new(peers.clone());

        // Create a peer for ourselves.
        trace!("Creating self-dial connection.");
        let (client_socket, peer_socket) = zbus::connection::socket::Channel::pair();
        let service_conn = connection::Builder::authenticated_socket(client_socket, guid.clone())?
            .p2p()
            .unique_name(fdo::BUS_NAME)?
            .name(fdo::BUS_NAME)?
            .serve_at(fdo::DBus::PATH, dbus)?
            .serve_at(fdo::Monitoring::PATH, monitoring)?
            .build()
            .await?;
        let peer_conn = connection::Builder::authenticated_socket(peer_socket, guid.clone())?
            .p2p()
            .build()
            .await?;

        peers.add_us(peer_conn).await;
        trace!("Self-dial connection created.");

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
                address: addr,
                peers,
                guid,
                next_id: 0,
                auth_mechanism,
                _self_conn: service_conn,
            },
        })
    }

    pub fn address(&self) -> &str {
        &self.inner.address
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            self.accept_next().await?;
        }
    }

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        #[cfg(unix)]
        if let Listener::Unix(unix_listener) = self.listener {
            let addr = unix_listener.local_addr()?;
            if let Some(path) = addr.as_pathname() {
                remove_file(path).await?;
            }
        }

        Ok(())
    }

    #[cfg(unix)]
    async fn unix_stream(unix: &Unix<'_>) -> Result<Listener> {
        // TODO: Use tokio::net::UnixListener directly once it supports abstract sockets:
        //
        // https://github.com/tokio-rs/tokio/issues/4610

        use std::os::unix::net::SocketAddr;
        use zbus::address::transport::UnixAddrKind;

        let addr = match unix.kind() {
            #[cfg(target_os = "linux")]
            UnixAddrKind::Abstract(name) => {
                use std::os::linux::net::SocketAddrExt;

                let addr = SocketAddr::from_abstract_name(name)?;
                info!(
                    "Listening on abstract UNIX socket `{}`.",
                    String::from_utf8_lossy(name)
                );

                addr
            }
            UnixAddrKind::Path(path) => {
                let addr = SocketAddr::from_pathname(path)?;
                info!(
                    "Listening on UNIX socket file `{}`.",
                    path.to_string_lossy()
                );

                addr
            }
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

    async fn tcp_stream(tcp: &Tcp<'_>) -> Result<Listener> {
        let Some(host) = tcp.host() else {
            bail!("No host= provided.");
        };
        let Some(port) = tcp.port() else {
            bail!("No port= provided.");
        };
        info!("Listening on `{}:{}`.", host, port);
        let address = (host, port);

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
        match &mut self.listener {
            #[cfg(unix)]
            Listener::Unix(listener) => {
                let (stream, addr) = listener.accept().await?;
                debug!("Accepted Unix connection from address `{:?}`", addr);
                Ok(stream.into())
            }
            Listener::Tcp(listener) => {
                let (stream, addr) = listener.accept().await?;
                debug!("Accepted TCP connection from address `{}`", addr);
                Ok(stream.into())
            }
        }
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

    fn next_id(&mut self) -> usize {
        self.inner.next_id += 1;

        self.inner.next_id
    }
}
