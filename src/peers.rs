use anyhow::{anyhow, bail, Context, Result};
use futures_util::{stream::StreamExt, SinkExt};
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    RwLock,
};
use tracing::{debug, trace, warn};
use zbus::{
    names::{BusName, OwnedUniqueName, UniqueName},
    MessageField, MessageFieldCode, MessageType,
};

use crate::{
    name_registry::{NameOwnerChanged, NameRegistry},
    peer::Peer,
    peer_stream::PeerStream,
};

#[derive(Debug)]
pub struct Peers {
    peers: RwLock<BTreeMap<OwnedUniqueName, Peer>>,
    name_registry: RwLock<NameRegistry>,
    name_changed_tx: Sender<NameOwnerChanged>,
}

impl Peers {
    pub fn new() -> (Self, Receiver<NameOwnerChanged>) {
        let (name_registry, name_changed_rx) = NameRegistry::new();
        (
            Self {
                peers: RwLock::new(BTreeMap::new()),
                name_changed_tx: name_registry.name_changed_tx().clone(),
                name_registry: RwLock::new(name_registry),
            },
            name_changed_rx,
        )
    }

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
                peers.insert(unique_name.clone(), peer);
                drop(peers);
                if let Err(e) = self
                    .name_changed_tx
                    .send(NameOwnerChanged {
                        name: BusName::from(unique_name.to_owned()).into(),
                        old_owner: None,
                        new_owner: Some(unique_name),
                    })
                    .await
                {
                    debug!("failed to send NameOwnerChanged: {e}");
                }
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
        mut peer_stream: PeerStream,
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

            match msg.message_type() {
                MessageType::Signal => self.broadcast_msg(msg).await,
                _ => match msg.fields()?.get_field(MessageFieldCode::Destination) {
                    Some(MessageField::Destination(dest)) => {
                        if let Err(e) = self.send_msg(msg.clone(), dest.clone()).await {
                            warn!("{}", e);
                        }
                    }
                    // PeerStream ensures a valid destination so this isn't exactly needed.
                    _ => bail!("invalid message: {:?}", msg),
                },
            };
        }

        // Stream is done means the peer disconnected. Remove it from the list of peers.
        self.name_registry_mut()
            .await
            .release_all(unique_name.clone())
            .await;
        if let Err(e) = self
            .name_changed_tx
            .send(NameOwnerChanged {
                name: BusName::from(unique_name.clone()).into(),
                old_owner: Some(unique_name.clone()),
                new_owner: None,
            })
            .await
        {
            debug!("failed to send NameOwnerChanged: {e}");
        }
        self.peers_mut().await.remove(&unique_name);

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
            Some(mut conn) => conn.send(msg).await.context("failed to send message")?,
            None => debug!("no peer for destination `{destination}`"),
        }

        Ok(())
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
