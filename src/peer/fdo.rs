use std::collections::HashSet;

use enumflags2::BitFlags;
use zbus::{
    dbus_interface,
    fdo::{self, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{BusName, OwnedBusName, OwnedUniqueName, OwnedWellKnownName},
    OwnedMatchRule,
};

use crate::name_registry::NameRegistry;

#[derive(Debug)]
pub(super) struct DBus {
    greeted: bool,
    unique_name: OwnedUniqueName,
    name_registry: NameRegistry,
    match_rules: HashSet<OwnedMatchRule>,
}

impl DBus {
    pub(super) fn new(unique_name: OwnedUniqueName, name_registry: NameRegistry) -> Self {
        Self {
            greeted: false,
            unique_name,
            name_registry,
            match_rules: HashSet::new(),
        }
    }

    pub(super) fn match_rules(&self) -> impl Iterator<Item = &OwnedMatchRule> {
        self.match_rules.iter()
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
    fn request_name(
        &self,
        name: OwnedWellKnownName,
        flags: BitFlags<RequestNameFlags>,
    ) -> RequestNameReply {
        self.name_registry
            .request_name(name, self.unique_name.clone(), flags)
    }

    /// Ask the message bus to release the method caller's claim to the given name.
    fn release_name(&self, name: OwnedWellKnownName) -> ReleaseNameReply {
        self.name_registry
            .release_name(name.into(), (&*self.unique_name).into())
    }

    /// Returns the unique connection name of the primary owner of the name given.
    fn get_name_owner(&self, name: OwnedBusName) -> fdo::Result<OwnedUniqueName> {
        match name.into_inner() {
            BusName::WellKnown(name) => self.name_registry.lookup(name).ok_or_else(|| {
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
