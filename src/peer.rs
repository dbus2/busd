use std::collections::HashSet;

use anyhow::Result;
use enumflags2::BitFlags;
use tokio::net::UnixStream;
use tracing::trace;
use zbus::{
    dbus_interface,
    fdo::{self, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{
        BusName, InterfaceName, OwnedBusName, OwnedUniqueName, OwnedWellKnownName, UniqueName,
    },
    zvariant::{ObjectPath, Structure},
    Connection, ConnectionBuilder, Guid, MatchRulePathSpec, MessageStream, OwnedMatchRule,
};

use crate::name_registry::NameRegistry;

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    conn: Connection,
    unique_name: OwnedUniqueName,
}

impl Peer {
    pub async fn new(
        guid: &Guid,
        id: usize,
        unix_stream: UnixStream,
        name_registry: NameRegistry,
    ) -> Result<Self> {
        let unique_name = OwnedUniqueName::try_from(format!(":dbuz.{id}")).unwrap();

        let conn = ConnectionBuilder::socket(unix_stream)
            .server(guid)
            .p2p()
            .serve_at(
                "/org/freedesktop/DBus",
                DBus::new(unique_name.clone(), name_registry),
            )?
            .name("org.freedesktop.DBus")?
            .unique_name("org.freedesktop.DBus")?
            .build()
            .await?;
        trace!("created: {:?}", conn);

        Ok(Self { conn, unique_name })
    }

    pub fn unique_name(&self) -> &OwnedUniqueName {
        &self.unique_name
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn stream(&self) -> MessageStream {
        MessageStream::from(&self.conn)
    }

    /// # Panics
    ///
    /// if header, SENDER or DESTINATION is not set.
    pub async fn interested(&self, msg: &zbus::Message) -> bool {
        let dbus_ref = self
            .conn
            .object_server()
            .interface::<_, DBus>("/org/freedesktop/DBus")
            .await
            .expect("DBus interface not found");
        let dbus = dbus_ref.get().await;
        let hdr = msg.header().expect("received message without header");

        dbus.match_rules.iter().any(|rule| {
            // Start with message type.
            if let Some(msg_type) = rule.msg_type() {
                if msg_type != msg.message_type() {
                    return false;
                }
            }

            // Then check sender.
            let sender = rule.sender().cloned().and_then(|name| match name {
                BusName::WellKnown(name) => dbus.name_registry.lookup(name).as_deref().cloned(),
                BusName::Unique(name) => Some(name),
            });
            if let Some(sender) = sender {
                if sender
                    != hdr
                        .sender()
                        .expect("SENDER field unset")
                        .expect("SENDER field unset")
                        .clone()
                {
                    return false;
                }
            }

            // The interface.
            if let Some(interface) = rule.interface() {
                match msg.interface().as_ref() {
                    Some(msg_interface) if interface != msg_interface => return false,
                    Some(_) => (),
                    None => return false,
                }
            }

            // The member.
            if let Some(member) = rule.member() {
                match msg.member().as_ref() {
                    Some(msg_member) if member != msg_member => return false,
                    Some(_) => (),
                    None => return false,
                }
            }

            // The destination.
            if let Some(destination) = rule.destination() {
                let msg_destination: UniqueName = match hdr
                    .destination()
                    .expect("DESTINATION field unset")
                    .expect("DESTINATION field unset")
                    .clone()
                {
                    BusName::WellKnown(name) => match dbus.name_registry.lookup(name) {
                        Some(name) => name.into(),
                        None => return false,
                    },
                    BusName::Unique(name) => name,
                };
                if destination != &msg_destination {
                    return false;
                }
            }

            // The path.
            if let Some(path_spec) = rule.path_spec() {
                let msg_path = match msg.path() {
                    Some(p) => p,
                    None => return false,
                };
                match path_spec {
                    MatchRulePathSpec::Path(path) if path != &msg_path => return false,
                    MatchRulePathSpec::PathNamespace(path_ns)
                        if !msg_path.starts_with(path_ns.as_str()) =>
                    {
                        return false;
                    }
                    MatchRulePathSpec::Path(_) | MatchRulePathSpec::PathNamespace(_) => (),
                }
            }

            // The arg0 namespace.
            if let Some(arg0_ns) = rule.arg0namespace() {
                if let Ok(arg0) = msg.body_unchecked::<InterfaceName<'_>>() {
                    if !arg0.starts_with(arg0_ns.as_str()) {
                        return false;
                    }
                } else {
                    return false;
                }
            }

            // Args
            let structure = match msg.body::<Structure<'_>>() {
                Ok(s) => s,
                Err(_) => return false,
            };
            let args = structure.fields();

            for (i, arg) in rule.args() {
                match args.get(*i as usize) {
                    Some(msg_arg) => match <&str>::try_from(msg_arg) {
                        Ok(msg_arg) if arg != msg_arg => return false,
                        Ok(_) => (),
                        Err(_) => return false,
                    },
                    None => return false,
                }
            }

            // Path args
            for (i, path) in rule.arg_paths() {
                match args.get(*i as usize) {
                    Some(msg_arg) => match <ObjectPath<'_>>::try_from(msg_arg) {
                        Ok(msg_arg) if *path != msg_arg => return false,
                        Ok(_) => (),
                        Err(_) => return false,
                    },
                    None => return false,
                }
            }

            true
        })
    }
}

#[derive(Debug)]
struct DBus {
    greeted: bool,
    unique_name: OwnedUniqueName,
    name_registry: NameRegistry,
    match_rules: HashSet<OwnedMatchRule>,
}

impl DBus {
    fn new(unique_name: OwnedUniqueName, name_registry: NameRegistry) -> Self {
        Self {
            greeted: false,
            unique_name,
            name_registry,
            match_rules: HashSet::new(),
        }
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
