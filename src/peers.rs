use anyhow::{anyhow, Context, Result};
use futures_util::{stream::StreamExt, SinkExt};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{trace, warn};
use zbus::{
    names::{BusName, OwnedUniqueName, UniqueName},
    zvariant::Type,
    MessageBuilder, MessageField, MessageFieldCode, MessageStream, MessageType,
};

use crate::{name_registry::NameRegistry, peer::Peer};

#[derive(Debug, Default)]
pub struct Peers {
    peers: RwLock<BTreeMap<OwnedUniqueName, Peer>>,
    name_registry: RwLock<NameRegistry>,
}

impl Peers {
    pub async fn add(self: &Arc<Self>, peer: Peer) {
        let unique_name = peer.unique_name().clone();
        let mut peers = self.peers.write().await;
        match peers.get(&unique_name) {
            Some(peer) => panic!(
                "Unique name `{}` re-used. We're in deep trouble if this happens",
                peer.unique_name()
            ),
            None => {
                let peer_stream = peer.stream();
                tokio::spawn(self.clone().serve_peer(peer_stream, unique_name.clone()));
                peers.insert(unique_name, peer);
            }
        }
    }

    pub async fn peers(&self) -> impl Deref<Target = BTreeMap<OwnedUniqueName, Peer>> + '_ {
        self.peers.read().await
    }

    pub async fn peers_mut(&self) -> impl DerefMut<Target = BTreeMap<OwnedUniqueName, Peer>> + '_ {
        self.peers.write().await
    }

    pub async fn name_registry(&self) -> impl Deref<Target = NameRegistry> + '_ {
        self.name_registry.read().await
    }

    pub async fn name_registry_mut(&self) -> impl DerefMut<Target = NameRegistry> + '_ {
        self.name_registry.write().await
    }

    async fn serve_peer(
        self: Arc<Self>,
        mut peer_stream: MessageStream,
        unique_name: OwnedUniqueName,
    ) -> Result<()> {
        while let Some(msg) = peer_stream.next().await {
            let msg = match msg {
                Ok(msg) => msg,
                Err(e) => {
                    warn!("Error: {}", e);

                    continue;
                }
            };
            let fields = match msg.message_type() {
                MessageType::MethodCall
                | MessageType::MethodReturn
                | MessageType::Error
                | MessageType::Signal => match msg.fields() {
                    Ok(fields) => fields,
                    Err(e) => {
                        warn!("failed to parse message: {}", e);

                        continue;
                    }
                },
                MessageType::Invalid => todo!(),
            };
            // Ensure sender field is present. If it is not we add it using the unique name of the peer.
            let (msg, fields) = match fields.get_field(MessageFieldCode::Sender) {
                Some(MessageField::Sender(sender)) if *sender == unique_name => {
                    (msg.clone(), fields)
                }
                Some(_) => {
                    warn!("failed to parse message: Invalid sender field");

                    continue;
                }
                None => {
                    let header = match msg.header() {
                        Ok(hdr) => hdr,
                        Err(e) => {
                            warn!("failed to parse message: {}", e);

                            continue;
                        }
                    };
                    let signature = match header.signature() {
                        Ok(Some(sig)) => sig.clone(),
                        Ok(None) => <()>::signature(),
                        Err(e) => {
                            warn!("Failed to parse signature from message: {}", e);

                            continue;
                        }
                    };
                    let body_bytes = match msg.body_as_bytes() {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            warn!("failed to parse message: {}", e);

                            continue;
                        }
                    };
                    let builder = MessageBuilder::from(header.clone()).sender(&unique_name)?;
                    let new_msg = match unsafe {
                        builder.build_raw_body(
                            body_bytes,
                            signature,
                            #[cfg(unix)]
                            msg.take_fds().iter().map(|fd| fd.as_raw_fd()).collect(),
                        )
                    } {
                        Ok(msg) => msg,
                        Err(e) => {
                            warn!("failed to parse message: {}", e);

                            continue;
                        }
                    };

                    // SAFETY: We take the fields verbatim from the original message so we can
                    // assume it has valid fields.
                    let fields = msg.fields().expect("Missing message header fields");

                    trace!("Added sender field to message: {:?}", new_msg);
                    (Arc::new(new_msg), fields)
                }
            };

            match fields.get_field(MessageFieldCode::Destination) {
                Some(MessageField::Destination(dest)) => {
                    if let Err(e) = self.send_msg(msg.clone(), dest.clone()).await {
                        warn!("{}", e);
                    }
                }
                Some(_) => {
                    warn!("failed to parse message: Missing destination");
                }
                None => {
                    if msg.message_type() == MessageType::Signal {
                        self.broadcast_msg(msg).await;
                    } else {
                        warn!("missing destination field");
                    }
                }
            };
        }

        // Stream is done means the peer disconnected. Remove it from the list of peers.
        self.peers_mut().await.remove(&unique_name);
        self.name_registry_mut()
            .await
            .release_all(unique_name.into());

        Ok(())
    }

    async fn send_msg(&self, msg: Arc<zbus::Message>, destination: BusName<'_>) -> Result<()> {
        trace!(
            "Forwarding message: {:?}, destination: {}",
            msg,
            destination
        );
        match destination {
            BusName::Unique(dest) => self.send_msg_to_unique_name(msg, dest.clone()).await,
            BusName::WellKnown(name) => {
                let dest = match self.name_registry().await.lookup(name.clone()) {
                    Some(dest) => dest,
                    None => {
                        return Err(anyhow!("unknown destination: {}", name));
                    }
                };
                self.send_msg_to_unique_name(msg, (&*dest).into()).await
            }
        }
    }

    async fn send_msg_to_unique_name(
        &self,
        msg: Arc<zbus::Message>,
        destination: UniqueName<'_>,
    ) -> Result<()> {
        let conn = self
            .peers
            .read()
            .await
            .get(destination.as_str())
            .map(|peer| peer.conn().clone());
        match conn {
            Some(mut conn) => conn.send(msg).await.context("failed to send message"),
            None => Err(anyhow!("no peer for destination `{}`", destination)),
        }
    }

    async fn broadcast_msg(&self, msg: Arc<zbus::Message>) {
        trace!("Braadcasting message: {:?}", msg);
        for peer in self.peers.read().await.values() {
            if !peer.interested(&msg).await {
                continue;
            }

            if let Err(e) = peer
                .conn()
                .send(msg.clone())
                .await
                .context("failed to send message")
            {
                warn!("Error sending message: {}", e);
            }
        }
    }
}
