use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use enumflags2::BitFlags;
use serde::Serialize;
use tokio::{spawn, sync::oneshot};
use tracing::{debug, warn};
use zbus::{
    dbus_interface,
    fdo::{
        ConnectionCredentials, Error, ReleaseNameReply, RequestNameFlags, RequestNameReply, Result,
    },
    names::{BusName, InterfaceName, OwnedBusName, OwnedUniqueName, UniqueName, WellKnownName},
    zvariant::{Optional, Signature, Type},
    Guid, MessageHeader, OwnedMatchRule, SignalContext,
};

use crate::{peer::Peer, peers::Peers};

#[derive(Debug)]
pub struct DBus {
    peers: Weak<Peers>,
    guid: Arc<Guid>,
}

impl DBus {
    pub const PATH: &str = "/org/freedesktop/DBus";
    pub const INTERFACE: &str = "org.freedesktop.DBus";

    pub fn new(peers: Arc<Peers>, guid: Arc<Guid>) -> Self {
        Self {
            peers: Arc::downgrade(&peers),
            guid,
        }
    }

    /// Helper for D-Bus methods that call a function on a peer.
    async fn call_mut_on_peer<F, R>(&self, func: F, hdr: MessageHeader<'_>) -> Result<R>
    where
        F: FnOnce(&mut Peer) -> Result<R>,
    {
        let name = msg_sender(&hdr);
        let peers = self.peers()?;
        let mut peers = peers.peers_mut().await;
        let peer = peers
            .get_mut(name.as_str())
            .ok_or_else(|| Error::NameHasNoOwner(format!("No such peer: {}", name)))?;

        func(peer)
    }

    fn peers(&self) -> Result<Arc<Peers>> {
        self.peers
            .upgrade()
            // Can it happen in any other situation than the bus shutting down?
            .ok_or_else(|| Error::Failed("Bus shutting down.".to_string()))
    }
}

#[dbus_interface(interface = "org.freedesktop.DBus")]
impl DBus {
    /// This is already called & handled and we only need to handle it once.
    async fn hello(
        &self,
        #[zbus(header)] hdr: MessageHeader<'_>,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> Result<HelloResponse> {
        let name = msg_sender(&hdr);
        let peers = self.peers()?;
        let mut peers = peers.peers_mut().await;
        let peer = peers
            .get_mut(name.as_str())
            .ok_or_else(|| Error::NameHasNoOwner(format!("No such peer: {}", name)))?;
        peer.hello().await?;

        // Notify name change in a separate task because we want:
        // 1. `Hello` to return ASAP and hence client connection to be esablished.
        // 2. The `Hello` response to arrive before the `NameAcquired` signal.
        let unique_name = peer.unique_name().clone();
        let (tx, rx) = oneshot::channel();
        let response = HelloResponse {
            name: unique_name.clone(),
            tx: Some(tx),
        };
        let ctxt = ctxt.to_owned();
        spawn(async move {
            if let Err(e) = rx.await {
                warn!("Failed to wait for `Hello` response: {e}");

                return;
            }
            let owner = UniqueName::from(unique_name);

            if let Err(e) = Self::name_owner_changed(
                &ctxt,
                owner.clone().into(),
                None.into(),
                Some(owner.clone()).into(),
            )
            .await
            {
                warn!("Failed to notify peers of name change: {}", e);
            }

            let ctxt = ctxt.set_destination(owner.clone().into());
            if let Err(e) = Self::name_acquired(&ctxt, owner.into()).await {
                warn!("Failed to send `NameAcquired` signal: {}", e);
            }
        });

        Ok(response)
    }

    /// Ask the message bus to assign the given name to the method caller.
    async fn request_name(
        &self,
        name: WellKnownName<'_>,
        flags: BitFlags<RequestNameFlags>,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> Result<RequestNameReply> {
        let unique_name = msg_sender(&hdr);
        let peers = self.peers()?;
        let (reply, name_owner_changed) = peers
            .name_registry_mut()
            .await
            .request_name(name, unique_name.clone(), flags)
            .await;
        if let Some(changed) = name_owner_changed {
            peers
                .notify_name_changes(changed)
                .await
                .map_err(|e| Error::Failed(e.to_string()))?;
        }

        Ok(reply)
    }

    /// Ask the message bus to release the method caller's claim to the given name.
    async fn release_name(
        &self,
        name: WellKnownName<'_>,
        #[zbus(header)] hdr: MessageHeader<'_>,
    ) -> Result<ReleaseNameReply> {
        let unique_name = msg_sender(&hdr);
        let peers = self.peers()?;
        let (reply, name_owner_changed) = peers
            .name_registry_mut()
            .await
            .release_name(name, unique_name.clone())
            .await;
        if let Some(changed) = name_owner_changed {
            peers
                .notify_name_changes(changed)
                .await
                .map_err(|e| Error::Failed(e.to_string()))?;
        }

        Ok(reply)
    }

    /// Returns the unique connection name of the primary owner of the name given.
    async fn get_name_owner(&self, name: BusName<'_>) -> Result<OwnedUniqueName> {
        let peers = self.peers()?;
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
        &self,
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
        &self,
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
        let peers = self.peers()?;
        let peers = peers.peers().await;
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
    fn get_id(&self) -> &Guid {
        &self.guid
    }

    /// Returns a list of all names that can be activated on the bus.
    fn list_activatable_names(&self) -> &[OwnedBusName] {
        // TODO: Return actual list when we support service activation.
        &[]
    }

    /// Returns a list of all currently-owned names on the bus.
    async fn list_names(&self) -> Result<Vec<OwnedBusName>> {
        let peers = self.peers()?;
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

        Ok(names)
    }

    /// List the connections currently queued for a bus name.
    async fn list_queued_owners(&self, name: WellKnownName<'_>) -> Result<Vec<OwnedUniqueName>> {
        self.peers()?
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

    /// Reload server configuration.
    fn reload_config(&self) -> Result<()> {
        // TODO: Implement when we support configuration.
        Err(Error::Failed(
            "No server configuration to reload.".to_string(),
        ))
    }

    /// Easter egg method.
    fn dune(&self) -> &str {
        "I must not fear. Fear is the mind-killer. Fear is the little-death that brings total \
        obliteration. I will face my fear. I will permit it to pass over me and through me. And \
        when it has gone past I will turn the inner eye to see its path. Where the fear has gone \
        there will be nothing. Only **I** will remain!"
    }

    //
    // Propertries
    //

    /// This property lists abstract “features” provided by the message bus, and can be used by
    /// clients to detect the capabilities of the message bus with which they are communicating.
    #[dbus_interface(property)]
    fn features(&self) -> &[&str] {
        &[]
    }

    /// This property lists interfaces provided by the `/org/freedesktop/DBus` object, and can be
    /// used by clients to detect the capabilities of the message bus with which they are
    /// communicating. Unlike the standard Introspectable interface, querying this property does not
    /// require parsing XML. This property was added in version 1.11.x of the reference
    /// implementation of the message bus.
    ///
    /// The standard `org.freedesktop.DBus` and `org.freedesktop.DBus.Properties` interfaces are not
    /// included in the value of this property, because their presence can be inferred from the fact
    /// that a method call on `org.freedesktop.DBus.Properties` asking for properties of
    /// `org.freedesktop.DBus` was successful. The standard `org.freedesktop.DBus.Peer` and
    /// `org.freedesktop.DBus.Introspectable` interfaces are not included in the value of this
    /// property either, because they do not indicate features of the message bus implementation.
    #[dbus_interface(property)]
    fn interfaces(&self) -> &[InterfaceName<'_>] {
        // TODO: List `org.freedesktop.DBus.Monitoring` when we support it.
        &[]
    }

    /// This signal indicates that the owner of a name has changed.
    ///
    /// It's also the signal to use to detect the appearance of new names on the bus.
    #[dbus_interface(signal)]
    pub async fn name_owner_changed(
        signal_ctxt: &SignalContext<'_>,
        name: BusName<'_>,
        old_owner: Optional<UniqueName<'_>>,
        new_owner: Optional<UniqueName<'_>>,
    ) -> zbus::Result<()>;

    /// This signal is sent to a specific application when it loses ownership of a name.
    #[dbus_interface(signal)]
    pub async fn name_lost(signal_ctxt: &SignalContext<'_>, name: BusName<'_>) -> zbus::Result<()>;

    /// This signal is sent to a specific application when it gains ownership of a name.
    #[dbus_interface(signal)]
    pub async fn name_acquired(
        signal_ctxt: &SignalContext<'_>,
        name: BusName<'_>,
    ) -> zbus::Result<()>;
}

/// Custom type for `Hello` method response.
///
/// We need to ensure that the `NameAcquired` signal is not sent out before the response so we
/// return this from the method and when zbus is done sending it, it will drop it and we can then
/// send the signal.
#[derive(Debug)]
struct HelloResponse {
    name: OwnedUniqueName,
    tx: Option<oneshot::Sender<()>>,
}

impl Serialize for HelloResponse {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.name.serialize(serializer)
    }
}

impl Type for HelloResponse {
    fn signature() -> Signature<'static> {
        UniqueName::signature()
    }
}

impl Drop for HelloResponse {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            if let Err(e) = tx.send(()) {
                debug!("{e:?}");
            }
        }
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
