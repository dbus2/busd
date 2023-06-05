use std::sync::Arc;

use enumflags2::BitFlags;
use zbus::{
    dbus_interface,
    fdo::{self, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{BusName, OwnedUniqueName, OwnedWellKnownName, UniqueName},
    MessageHeader, OwnedMatchRule,
};

use crate::{peer::Peer, peers::Peers};

#[derive(Debug)]
pub(super) struct DBus {
    peers: Arc<Peers>,
}

impl DBus {
    pub(super) fn new(peers: Arc<Peers>) -> Self {
        Self { peers }
    }

    /// Helper for D-Bus methods that call a function on a peer.
    async fn call_mut_on_peer<F, R>(&mut self, func: F, hdr: MessageHeader<'_>) -> fdo::Result<R>
    where
        F: FnOnce(&mut Peer) -> fdo::Result<R>,
    {
        let name = msg_sender(&hdr);
        let mut peers = self.peers.peers_mut().await;
        let peer = peers
            .get_mut(name.as_str())
            .ok_or_else(|| fdo::Error::NameHasNoOwner(format!("No such peer: {}", name)))?;

        func(peer)
    }
}

#[dbus_interface(interface = "org.freedesktop.DBus")]
impl DBus {
    /// Returns the unique name assigned to the connection.
    async fn hello(
        &mut self,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> fdo::Result<OwnedUniqueName> {
        self.call_mut_on_peer(move |peer| peer.hello(), hdr).await
    }

    /// Ask the message bus to assign the given name to the method caller.
    async fn request_name(
        &self,
        name: OwnedWellKnownName,
        flags: BitFlags<RequestNameFlags>,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> fdo::Result<RequestNameReply> {
        let unique_name = msg_sender(&hdr);
        Ok(self.peers.name_registry_mut().await.request_name(
            name,
            unique_name.clone().into(),
            flags,
        ))
    }

    /// Ask the message bus to release the method caller's claim to the given name.
    async fn release_name(
        &self,
        name: OwnedWellKnownName,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> fdo::Result<ReleaseNameReply> {
        let unique_name = msg_sender(&hdr);
        Ok(self
            .peers
            .name_registry_mut()
            .await
            .release_name(name.into(), unique_name.clone()))
    }

    /// Returns the unique connection name of the primary owner of the name given.
    async fn get_name_owner(&self, name: BusName<'_>) -> fdo::Result<OwnedUniqueName> {
        let peers = &self.peers;

        match name {
            BusName::WellKnown(name) => peers.name_registry().await.lookup(name).ok_or_else(|| {
                fdo::Error::NameHasNoOwner("Name is not owned by anyone. Take it!".to_string())
            }),
            BusName::Unique(name) => {
                if peers.peers().await.contains_key(&*name) {
                    Ok(name.into())
                } else {
                    Err(fdo::Error::NameHasNoOwner(
                        "Name is not owned by anyone.".to_string(),
                    ))
                }
            }
        }
    }

    /// Adds a match rule to match messages going through the message bus
    async fn add_match(
        &mut self,
        rule: OwnedMatchRule,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> fdo::Result<()> {
        self.call_mut_on_peer(
            move |peer| {
                peer.add_match_rule(rule);

                Ok(())
            },
            hdr,
        )
        .await
    }

    /// Removes the first rule that matches.
    async fn remove_match(
        &mut self,
        rule: OwnedMatchRule,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> fdo::Result<()> {
        self.call_mut_on_peer(move |peer| peer.remove_match_rule(rule), hdr)
            .await
    }

    /// Returns auditing data used by Solaris ADT, in an unspecified binary format.
    fn get_adt_audit_session_data(&self, _bus_name: BusName<'_>) -> fdo::Result<Vec<u8>> {
        Err(fdo::Error::NotSupported("Solaris really?".to_string()))
    }
}

/// Helper for getting the peer name from a message header.
fn msg_sender<'h>(hdr: &'h MessageHeader<'h>) -> &'h UniqueName<'h> {
    // SAFETY: The bus (that's us!) is supposed to ensure a valid sender on the message.
    hdr.sender()
        .ok()
        .flatten()
        .expect("Missing `sender` header")
}
