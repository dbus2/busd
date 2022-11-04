use anyhow::Result;
use futures_util::{stream::StreamExt, SinkExt};
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};
use tracing::warn;
use zbus::{
    names::{BusName, OwnedUniqueName},
    MessageField, MessageFieldCode, MessageStream, MessageType,
};

use crate::peer::Peer;

#[derive(Clone, Debug)]
pub struct Peers(Arc<RwLock<BTreeMap<OwnedUniqueName, Peer>>>);

impl Peers {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(BTreeMap::new())))
    }

    pub fn add(&self, peer: Peer) {
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
