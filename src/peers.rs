use anyhow::{bail, Context, Result};
use futures_util::{stream::StreamExt, SinkExt};
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};
use zbus::{
    names::{BusName, OwnedUniqueName, UniqueName},
    zvariant::Optional,
    AuthMechanism, Guid, MessageBuilder, MessageField, MessageFieldCode, MessageType, Socket,
};

use crate::{
    fdo,
    name_registry::{NameOwnerChanged, NameRegistry},
    peer::{Peer, Stream},
};

#[derive(Debug)]
pub struct Peers {
    peers: RwLock<BTreeMap<OwnedUniqueName, Peer>>,
    name_registry: RwLock<NameRegistry>,
}

impl Peers {
    pub fn new() -> Arc<Self> {
        let name_registry = NameRegistry::default();

        Arc::new(Self {
            peers: RwLock::new(BTreeMap::new()),
            name_registry: RwLock::new(name_registry),
        })
    }

    pub async fn add(
        self: &Arc<Self>,
        guid: &Arc<Guid>,
        id: Option<usize>,
        socket: Box<dyn Socket + 'static>,
        auth_mechanism: AuthMechanism,
    ) -> Result<()> {
        let mut peers = self.peers_mut().await;
        let peer = Peer::new(guid.clone(), id, socket, auth_mechanism).await?;
        let unique_name = peer.unique_name().clone();
        match peers.get(&unique_name) {
            Some(peer) => panic!(
                "Unique name `{}` re-used. We're in deep trouble if this happens",
                peer.unique_name()
            ),
            None => {
                let peer_stream = peer.stream();
                tokio::spawn(self.clone().serve_peer(peer_stream, unique_name.clone()));
                peers.insert(unique_name.clone(), peer);
            }
        }

        Ok(())
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

    pub async fn notify_name_changes(&self, name_owner_changed: NameOwnerChanged) -> Result<()> {
        let name = BusName::from(name_owner_changed.name);
        let old_owner = name_owner_changed.old_owner.map(UniqueName::from);
        let new_owner = name_owner_changed.new_owner.map(UniqueName::from);

        // First broadcast the name change signal.
        let msg = MessageBuilder::signal(fdo::DBUS_PATH, fdo::DBUS_INTERFACE, "NameOwnerChanged")
            .unwrap()
            .sender(fdo::BUS_NAME)
            .unwrap()
            .build(&(
                &name,
                Optional::from(old_owner.clone()),
                Optional::from(new_owner.clone()),
            ))?;
        self.broadcast_msg(Arc::new(msg)).await;

        // Now unicast the appropriate signal to the old and new owners.
        if let Some(old_owner) = old_owner {
            let msg = MessageBuilder::signal(fdo::DBUS_PATH, fdo::DBUS_INTERFACE, "NameLost")
                .unwrap()
                .sender(fdo::BUS_NAME)
                .unwrap()
                .destination(old_owner.clone())
                .unwrap()
                .build(&name)?;
            if let Err(e) = self
                .send_msg_to_unique_name(Arc::new(msg), old_owner.clone())
                .await
            {
                warn!("Couldn't notify inexistant peer {old_owner} about loosing name {name}: {e}")
            }
        }
        if let Some(new_owner) = new_owner {
            let msg = MessageBuilder::signal(fdo::DBUS_PATH, fdo::DBUS_INTERFACE, "NameAcquired")
                .unwrap()
                .sender(fdo::BUS_NAME)
                .unwrap()
                .destination(new_owner.clone())
                .unwrap()
                .build(&name)?;
            if let Err(e) = self
                .send_msg_to_unique_name(Arc::new(msg), new_owner.clone())
                .await
            {
                warn!("Couldn't notify peer {new_owner} about acquiring name {name}: {e}")
            }
        }

        Ok(())
    }

    async fn serve_peer(
        self: Arc<Self>,
        mut peer_stream: Stream,
        unique_name: OwnedUniqueName,
    ) -> Result<()> {
        while let Some(msg) = peer_stream.next().await {
            let msg = match msg {
                Ok(msg) => msg,
                Err(e) => {
                    debug!("{e}");

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
                    // peer::Stream ensures a valid destination so this isn't exactly needed.
                    _ => bail!("invalid message: {:?}", msg),
                },
            };
        }

        // Stream is done means the peer disconnected. Remove it from the list of peers.
        self.peers_mut().await.remove(&unique_name);
        let names_changes = self
            .name_registry_mut()
            .await
            .release_all(unique_name.inner().clone())
            .await;
        for changed in names_changes {
            self.notify_name_changes(changed).await?;
        }
        self.notify_name_changes(NameOwnerChanged {
            name: BusName::from(unique_name.clone()).into(),
            old_owner: Some(unique_name.clone()),
            new_owner: None,
        })
        .await?;

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
                    None => bail!("unknown destination: {}", name),
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
        trace!("Broadcasting message: {:?}", msg);
        let name_registry = self.name_registry().await;
        for peer in self.peers.read().await.values() {
            if !peer.interested(&msg, &name_registry) {
                trace!("Peer {} not interested in {msg:?}", peer.unique_name());
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
