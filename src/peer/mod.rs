mod stream;
pub use stream::*;

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use tracing::trace;
use zbus::{
    names::{BusName, OwnedUniqueName},
    AuthMechanism, Connection, ConnectionBuilder, Guid, OwnedMatchRule, Socket,
};

use crate::{
    fdo::{self, DBus},
    name_registry::NameRegistry,
    peers::Peers,
};

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    conn: Connection,
    unique_name: OwnedUniqueName,
    match_rules: HashSet<OwnedMatchRule>,
}

impl Peer {
    pub async fn new(
        guid: Arc<Guid>,
        id: Option<usize>,
        socket: Box<dyn Socket + 'static>,
        auth_mechanism: AuthMechanism,
        peers: Arc<Peers>,
    ) -> Result<Self> {
        let unique_name = match id {
            Some(id) => OwnedUniqueName::try_from(format!(":busd.{id}")).unwrap(),
            None => OwnedUniqueName::try_from(fdo::BUS_NAME).unwrap(),
        };
        let dbus = DBus::new(unique_name.clone(), peers.clone(), guid.clone());
        let conn = ConnectionBuilder::socket(socket)
            .server(&guid)
            .p2p()
            .auth_mechanisms(&[auth_mechanism])
            .unique_name(fdo::BUS_NAME)?
            .name(fdo::BUS_NAME)?
            .serve_at(fdo::DBUS_PATH, dbus)?
            .build()
            .await?;
        trace!("created: {:?}", conn);

        Ok(Self {
            conn,
            unique_name,
            match_rules: HashSet::new(),
        })
    }

    pub fn unique_name(&self) -> &OwnedUniqueName {
        &self.unique_name
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn stream(&self) -> Stream {
        Stream::for_peer(self)
    }

    /// # Panics
    ///
    /// if header, SENDER or DESTINATION is not set.
    pub async fn interested(&self, msg: &zbus::Message, name_registry: &NameRegistry) -> bool {
        let hdr = msg.header().expect("received message without header");

        let ret = self.match_rules.iter().any(|rule| {
            // First make use of zbus API
            match rule.matches(msg) {
                Ok(false) => return false,
                Ok(true) => (),
                Err(e) => {
                    tracing::warn!("error matching rule: {}", e);

                    return false;
                }
            }

            // Then match sender and destination involving well-known names, manually.
            if let Some(sender) = rule.sender().cloned().and_then(|name| match name {
                BusName::WellKnown(name) => name_registry.lookup(name).as_deref().cloned(),
                // Unique name is already taken care of by the zbus API.
                BusName::Unique(_) => None,
            }) {
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

            // The destination.
            if let Some(destination) = rule.destination() {
                match hdr
                    .destination()
                    .expect("DESTINATION field unset")
                    .expect("DESTINATION field unset")
                    .clone()
                {
                    BusName::WellKnown(name) => match name_registry.lookup(name) {
                        Some(name) if name == *destination => (),
                        Some(_) => return false,
                        None => return false,
                    },
                    // Unique name is already taken care of by the zbus API.
                    BusName::Unique(_) => {}
                }
            }

            true
        });

        ret
    }

    pub fn add_match_rule(&mut self, rule: OwnedMatchRule) {
        self.match_rules.insert(rule);
    }

    /// Remove the first rule that matches.
    pub fn remove_match_rule(&mut self, rule: OwnedMatchRule) -> zbus::fdo::Result<()> {
        if !self.match_rules.remove(&rule) {
            return Err(zbus::fdo::Error::MatchRuleNotFound(
                "No such match rule".to_string(),
            ));
        }

        Ok(())
    }
}
