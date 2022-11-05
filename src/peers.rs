use anyhow::{anyhow, Context, Result};
use futures_util::{stream::StreamExt, SinkExt};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::warn;
use zbus::{
    names::{BusName, OwnedUniqueName, UniqueName},
    MessageField, MessageFieldCode, MessageStream, MessageType,
};

use crate::{name_registry::NameRegistry, peer::Peer};

#[derive(Clone, Debug)]
pub struct Peers {
    peers: Arc<RwLock<BTreeMap<OwnedUniqueName, Peer>>>,
    name_registry: NameRegistry,
}

impl Peers {
    pub fn new(name_registry: NameRegistry) -> Self {
        Self {
            peers: Arc::new(RwLock::new(BTreeMap::new())),
            name_registry,
        }
    }

    pub async fn add(&self, peer: Peer) {
        let unique_name = peer.unique_name().clone();
        let mut peers = self.peers.write().await;
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
                                warn!("missing destination field");
                            }
                        };
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

    async fn send_msg(&self, msg: Arc<zbus::Message>, destination: BusName<'_>) -> Result<()> {
        match destination {
            BusName::Unique(dest) => self.send_msg_to_unique_name(msg, dest.clone().into()).await,
            BusName::WellKnown(name) => {
                let dest = match self.name_registry.lookup(name.clone()) {
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
}
