use anyhow::Result;
use futures_util::{stream::StreamExt, SinkExt};
use nix::unistd::Uid;
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs::remove_file;
use tracing::{debug, warn};
use zbus::{
    names::{BusName, OwnedUniqueName},
    Guid, MessageField, MessageFieldCode, MessageStream, MessageType,
};

use crate::{name_registry::NameRegistry, peer::Peer};

/// The bus.
#[derive(Debug)]
pub struct Bus {
    peers: Peers,
    listener: tokio::net::UnixListener,
    guid: Guid,
    socket_path: PathBuf,
    next_id: usize,
    name_registry: NameRegistry,
}

impl Bus {
    pub async fn new(socket_path: Option<&Path>) -> Result<Self> {
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

        Ok(Self {
            listener: tokio::net::UnixListener::bind(&socket_path)?,
            peers: Peers::new(),
            guid: Guid::generate(),
            socket_path,
            next_id: 0,
            name_registry: NameRegistry::default(),
        })
    }

    pub async fn run(&mut self) {
        while let Ok((unix_stream, addr)) = self.listener.accept().await {
            debug!("Accepted connection from {:?}", addr);
            match Peer::new(
                &self.guid,
                self.next_id,
                unix_stream,
                self.name_registry.clone(),
            )
            .await
            {
                Ok(peer) => self.peers.add(peer),
                Err(e) => warn!("Failed to establish connection: {}", e),
            }
            self.next_id += 1;
        }
    }

    // AsyncDrop would have been nice!
    pub async fn cleanup(self) -> Result<()> {
        remove_file(&self.socket_path).await.map_err(Into::into)
    }
}

#[derive(Clone, Debug)]
struct Peers(Arc<RwLock<BTreeMap<OwnedUniqueName, Peer>>>);

impl Peers {
    fn new() -> Self {
        Self(Arc::new(RwLock::new(BTreeMap::new())))
    }

    fn add(&self, peer: Peer) {
        let unique_name = peer.unique_name().clone();
        let mut peers = self.0.write();
        match peers.get(&unique_name) {
            Some(peer) => panic!(
                "Unique name `{}` re-used. We're in deep trouble if this happens",
                peer.unique_name()
            ),
            None => {
                let peer_stream = peer.stream();
                tokio::spawn(self.clone().serve_peer(peer_stream));
                peers.insert(unique_name, peer);
            }
        }
    }

    async fn serve_peer(self, mut peer_stream: MessageStream) -> Result<()> {
        while let Some(msg) = peer_stream.next().await {
            match msg {
                Ok(msg) => match msg.message_type() {
                    MessageType::MethodCall | MessageType::MethodReturn | MessageType::Error => {
                        let fields = match msg.fields() {
                            Ok(fields) => fields,
                            Err(e) => {
                                warn!("failed to parse message: {}", e);
                                continue;
                            }
                        };
                        let dest = match fields.get_field(MessageFieldCode::Destination) {
                            Some(MessageField::Destination(BusName::Unique(dest))) => dest,
                            Some(MessageField::Destination(BusName::WellKnown(name))) => {
                                warn!("destination `{name}` is well-known name. Only unique names supported");
                                continue;
                            }
                            Some(_) => {
                                warn!("failed to parse message: Missing destination");
                                continue;
                            }
                            None => {
                                warn!("missing destination field");
                                continue;
                            }
                        };
                        let conn = self
                            .0
                            .read()
                            .get(dest.as_str())
                            .map(|peer| peer.conn().clone());
                        match conn {
                            Some(mut conn) => {
                                if let Err(e) = conn.send((*msg).clone()).await {
                                    warn!("failed to send message: {}", e);
                                }
                            }
                            None => {
                                warn!("no peer for destination `{}`", dest);
                            }
                        }
                    }
                    MessageType::Signal => todo!(),
                    MessageType::Invalid => todo!(),
                },
                Err(e) => {
                    warn!("Error: {:?}", e);
                }
            }
        }

        Ok(())
    }
}
