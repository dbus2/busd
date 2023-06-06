use std::{collections::HashMap, sync::Arc};

use enumflags2::BitFlags;
use zbus::{
    dbus_interface,
    fdo::{
        ConnectionCredentials, Error, ReleaseNameReply, RequestNameFlags, RequestNameReply, Result,
    },
    names::{
        BusName, OwnedBusName, OwnedUniqueName, OwnedWellKnownName, UniqueName, WellKnownName,
    },
    Guid, MessageHeader, OwnedMatchRule,
};

use crate::{peer::Peer, peers::Peers};

#[derive(Debug)]
pub(super) struct DBus {
    peers: Arc<Peers>,
    guid: Arc<Guid>,
}

impl DBus {
    pub(super) fn new(peers: Arc<Peers>, guid: Arc<Guid>) -> Self {
        Self { peers, guid }
    }

    /// Helper for D-Bus methods that call a function on a peer.
    async fn call_mut_on_peer<F, R>(&mut self, func: F, hdr: MessageHeader<'_>) -> Result<R>
    where
        F: FnOnce(&mut Peer) -> Result<R>,
    {
        let name = msg_sender(&hdr);
        let mut peers = self.peers.peers_mut().await;
        let peer = peers
            .get_mut(name.as_str())
            .ok_or_else(|| Error::NameHasNoOwner(format!("No such peer: {}", name)))?;

        func(peer)
    }
}

#[dbus_interface(interface = "org.freedesktop.DBus")]
impl DBus {
    /// Returns the unique name assigned to the connection.
    async fn hello(&mut self, #[zbus(header)] hdr: MessageHeader<'_>) -> Result<OwnedUniqueName> {
        self.call_mut_on_peer(move |peer| peer.hello(), hdr).await
    }

    /// Ask the message bus to assign the given name to the method caller.
    async fn request_name(
        &self,
        name: OwnedWellKnownName,
        flags: BitFlags<RequestNameFlags>,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> Result<RequestNameReply> {
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
    ) -> Result<ReleaseNameReply> {
        let unique_name = msg_sender(&hdr);
        Ok(self
            .peers
            .name_registry_mut()
            .await
            .release_name(name.into(), unique_name.clone()))
    }

    /// Returns the unique connection name of the primary owner of the name given.
    async fn get_name_owner(&self, name: BusName<'_>) -> Result<OwnedUniqueName> {
        let peers = &self.peers;

        match name {
            BusName::WellKnown(name) => peers.name_registry().await.lookup(name).ok_or_else(|| {
                Error::NameHasNoOwner("Name is not owned by anyone. Take it!".to_string())
            }),
            BusName::Unique(name) => {
                if peers.peers().await.contains_key(&*name) {
                    Ok(name.into())
                } else {
                    Err(Error::NameHasNoOwner(
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
    ) -> Result<()> {
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
    ) -> Result<()> {
        self.call_mut_on_peer(move |peer| peer.remove_match_rule(rule), hdr)
            .await
    }

    /// Returns auditing data used by Solaris ADT, in an unspecified binary format.
    fn get_adt_audit_session_data(&self, _bus_name: BusName<'_>) -> Result<Vec<u8>> {
        Err(Error::NotSupported("Solaris really?".to_string()))
    }

    /// Returns as many credentials as possible for the process connected to the server.
    async fn get_connection_credentials(
        &self,
        bus_name: BusName<'_>,
    ) -> Result<ConnectionCredentials> {
        let owner = self.get_name_owner(bus_name.clone()).await?;
        let peers = self.peers.peers().await;
        let peer = peers
            .get(&owner)
            .ok_or_else(|| Error::Failed(format!("Peer `{}` not found", bus_name)))?;

        peer.conn().peer_credentials().await.map_err(|e| {
            Error::Failed(format!(
                "Failed to get peer credentials for `{}`: {}",
                bus_name, e
            ))
        })
    }

    /// Returns the security context used by SELinux, in an unspecified format.
    #[dbus_interface(name = "GetConnectionSELinuxSecurityContext")]
    async fn get_connection_selinux_security_context(
        &self,
        bus_name: BusName<'_>,
    ) -> Result<Vec<u8>> {
        self.get_connection_credentials(bus_name)
            .await
            .and_then(|c| {
                c.into_linux_security_label().ok_or_else(|| {
                    Error::SELinuxSecurityContextUnknown("Unimplemented".to_string())
                })
            })
    }

    /// Returns the Unix process ID of the process connected to the server.
    #[dbus_interface(name = "GetConnectionUnixProcessID")]
    async fn get_connection_unix_process_id(&self, bus_name: BusName<'_>) -> Result<u32> {
        self.get_connection_credentials(bus_name.clone())
            .await
            .and_then(|c| {
                c.process_id().ok_or_else(|| {
                    Error::UnixProcessIdUnknown(format!(
                        "Could not determine Unix user ID of `{bus_name}`"
                    ))
                })
            })
    }

    /// Returns the Unix user ID of the process connected to the server.
    async fn get_connection_unix_user(&self, bus_name: BusName<'_>) -> Result<u32> {
        self.get_connection_credentials(bus_name.clone())
            .await
            .and_then(|c| {
                c.unix_user_id().ok_or_else(|| {
                    Error::Failed(format!("Could not determine Unix user ID of `{bus_name}`"))
                })
            })
    }

    /// Gets the unique ID of the bus.
    fn get_id(&self) -> Arc<Guid> {
        self.guid.clone()
    }

    /// Returns a list of all names that can be activated on the bus.
    fn list_activatable_names(&self) -> &'static [OwnedBusName] {
        // TODO: Return actual list when we support service activation.
        &[]
    }

    /// Returns a list of all currently-owned names on the bus.
    async fn list_names(&self) -> Vec<OwnedBusName> {
        let peers = &self.peers;
        let mut names: Vec<_> = peers
            .peers()
            .await
            .keys()
            .cloned()
            .map(|n| BusName::Unique(n.into()).into())
            .collect();
        names.extend(
            peers
                .name_registry()
                .await
                .all_names()
                .keys()
                .map(|n| BusName::WellKnown(n.into()).into()),
        );

        names
    }

    /// List the connections currently queued for a bus name.
    async fn list_queued_owners(&self, name: WellKnownName<'_>) -> Result<Vec<OwnedUniqueName>> {
        self.peers
            .name_registry()
            .await
            .waiting_list(name)
            .ok_or_else(|| {
                Error::NameHasNoOwner("Name is not owned by anyone. Take it!".to_string())
            })
            .map(|owners| owners.map(|o| o.unique_name()).cloned().collect())
    }

    /// Checks if the specified name exists (currently has an owner).
    async fn name_has_owner(&self, name: BusName<'_>) -> Result<bool> {
        match self.get_name_owner(name).await {
            Ok(_) => Ok(true),
            Err(Error::NameHasNoOwner(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Tries to launch the executable associated with a name (service activation).
    fn start_service_by_name(&self, _name: WellKnownName<'_>, _flags: u32) -> Result<u32> {
        // TODO: Implement when we support service activation.
        Err(Error::Failed(
            "Service activation not yet supported".to_string(),
        ))
    }

    /// This method adds to or modifies that environment when activating services.
    fn update_activation_environment(&self, _environment: HashMap<&str, &str>) -> Result<()> {
        // TODO: Implement when we support service activation.
        Err(Error::Failed(
            "Service activation not yet supported".to_string(),
        ))
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
