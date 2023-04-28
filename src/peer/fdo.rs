use std::{
    collections::HashSet,
    sync::{Arc, Weak},
};

use enumflags2::BitFlags;
use zbus::{
    dbus_interface,
    fdo::{self, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{BusName, OwnedBusName, OwnedUniqueName, OwnedWellKnownName},
    OwnedMatchRule,
};

use crate::peers::Peers;

#[derive(Debug)]
pub(super) struct DBus {
    greeted: bool,
    unique_name: OwnedUniqueName,
    match_rules: HashSet<OwnedMatchRule>,
    peers: Weak<Peers>,
}

impl DBus {
    pub(super) fn new(unique_name: OwnedUniqueName, peers: Weak<Peers>) -> Self {
        Self {
            greeted: false,
            unique_name,
            match_rules: HashSet::new(),
            peers,
        }
    }

    pub(super) fn match_rules(&self) -> impl Iterator<Item = &OwnedMatchRule> {
        self.match_rules.iter()
    }

    fn peers(&self) -> fdo::Result<Arc<Peers>> {
        self.peers
            .upgrade()
            // Can it happen in any other situation than the bus shutting down?
            .ok_or_else(|| fdo::Error::Failed("Bus shutting down.".to_string()))
    }
}

#[dbus_interface(interface = "org.freedesktop.DBus")]
impl DBus {
    /// Returns the unique name assigned to the connection.
    async fn hello(&mut self) -> fdo::Result<OwnedUniqueName> {
        if self.greeted {
            return Err(fdo::Error::Failed(
                "Can only call `Hello` method once".to_string(),
            ));
        }
        self.greeted = true;

        Ok(self.unique_name.clone())
    }

    /// Ask the message bus to assign the given name to the method caller.
    async fn request_name(
        &self,
        name: OwnedWellKnownName,
        flags: BitFlags<RequestNameFlags>,
    ) -> fdo::Result<RequestNameReply> {
        Ok(self.peers()?.name_registry_mut().await.request_name(
            name,
            self.unique_name.clone(),
            flags,
        ))
    }

    /// Ask the message bus to release the method caller's claim to the given name.
    async fn release_name(&self, name: OwnedWellKnownName) -> fdo::Result<ReleaseNameReply> {
        Ok(self
            .peers()?
            .name_registry_mut()
            .await
            .release_name(name.into(), (&*self.unique_name).into()))
    }

    /// Returns the unique connection name of the primary owner of the name given.
    async fn get_name_owner(&self, name: OwnedBusName) -> fdo::Result<OwnedUniqueName> {
        let peers = self.peers()?;

        match name.into_inner() {
            BusName::WellKnown(name) => peers.name_registry().await.lookup(name).ok_or_else(|| {
                fdo::Error::NameHasNoOwner("Name is not owned by anyone. Take it!".to_string())
            }),
            // FIXME: Not good enough. We need to check if name is actually owned.
            BusName::Unique(name) => Ok(name.into()),
        }
    }

    /// Adds a match rule to match messages going through the message bus
    fn add_match(&mut self, rule: OwnedMatchRule) {
        self.match_rules.insert(rule);
    }

    /// Removes the first rule that matches.
    fn remove_match(&mut self, rule: OwnedMatchRule) -> fdo::Result<()> {
        if !self.match_rules.remove(&rule) {
            return Err(fdo::Error::MatchRuleNotFound(
                "No such match rule".to_string(),
            ));
        }

        Ok(())
    }
}
