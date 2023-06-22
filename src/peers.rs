use anyhow::{anyhow, bail, Context, Result};
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
    AuthMechanism, Guid, MessageBuilder, MessageField, MessageFieldCode, MessageType,
    SignalContext, Socket,
};

use crate::{
    fdo::DBus,
    name_registry::{NameOwnerChanged, NameRegistry},
    peer::Peer,
    peer_stream::PeerStream,
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
        id: usize,
        socket: Box<dyn Socket + 'static>,
        auth_mechanism: AuthMechanism,
    ) -> Result<()> {
        let mut peers = self.peers_mut().await;
        let peer = Peer::new(guid.clone(), id, socket, auth_mechanism, self.clone()).await?;
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
        let msg = MessageBuilder::signal(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
        )
        .unwrap()
        .sender("org.freedesktop.DBus")
        .unwrap()
        .build(&(
            &name,
            Optional::from(old_owner.clone()),
            Optional::from(new_owner.clone()),
        ))?;
        self.broadcast_msg(Arc::new(msg)).await;

        // Now unicast the appropriate signal to the old and new owners.
        let peers = self.peers().await;
        if let Some(old_owner) = old_owner {
            match peers.get(&*old_owner) {
                Some(peer) => {
                    let signal_ctxt = SignalContext::new(peer.conn(), "/org/freedesktop/DBus")
                        .unwrap()
                        .set_destination(old_owner.into());
                    DBus::name_lost(&signal_ctxt, name.clone()).await?;
                }
                None => {
                    warn!("Couldn't notify inexistant peer {old_owner} about loosing name {name}")
                }
            }
        }
        if let Some(new_owner) = new_owner {
            match peers.get(&*new_owner) {
                Some(peer) => {
                    let signal_ctxt = SignalContext::new(peer.conn(), "/org/freedesktop/DBus")
                        .unwrap()
                        .set_destination(new_owner.into());
                    DBus::name_acquired(&signal_ctxt, name.clone()).await?;
                }
                None => {
                    warn!("Couldn't notify inexistant peer {new_owner} about acquiring name {name}")
                }
            }
        }

        Ok(())
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
        self.peers_mut().await.remove(&unique_name);
        let names_changes = self
            .name_registry_mut()
            .await
            .release_all(unique_name.clone())
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
        let name_registry = self.name_registry().await;
        for peer in self.peers.read().await.values() {
            if !peer.interested(&msg, &name_registry).await {
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
