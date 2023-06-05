use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use tracing::trace;
use zbus::{
    fdo,
    names::{BusName, OwnedUniqueName},
    AuthMechanism, Connection, ConnectionBuilder, Guid, MessageStream, OwnedMatchRule, Socket,
};

use crate::{name_registry::NameRegistry, peers::Peers};

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    greeted: bool,
    conn: Connection,
    unique_name: OwnedUniqueName,
    name_registry: NameRegistry,
    match_rules: HashSet<OwnedMatchRule>,
}

impl Peer {
    pub async fn new(
        guid: &Guid,
        id: Option<usize>,
        socket: Box<dyn Socket + 'static>,
        auth_mechanism: AuthMechanism,
        peers: Arc<Peers>,
    ) -> Result<Self> {
        let unique_name = match id {
            Some(id) => OwnedUniqueName::try_from(format!(":busd.{id}")).unwrap(),
            None => OwnedUniqueName::try_from("org.freedesktop.DBus").unwrap(),
        };

        let conn = ConnectionBuilder::socket(socket)
            .server(guid)
            .p2p()
            .auth_mechanisms(&[auth_mechanism])
            .build()
            .await?;
        trace!("created: {:?}", conn);

        let name_registry = peers.name_registry().await.clone();
        Ok(Self {
            conn,
            unique_name,
            name_registry,
            match_rules: HashSet::new(),
            greeted: false,
        })
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
                BusName::WellKnown(name) => self.name_registry.lookup(name).as_deref().cloned(),
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
                    BusName::WellKnown(name) => match self.name_registry.lookup(name) {
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
    pub fn remove_match_rule(&mut self, rule: OwnedMatchRule) -> fdo::Result<()> {
        if !self.match_rules.remove(&rule) {
            return Err(fdo::Error::MatchRuleNotFound(
                "No such match rule".to_string(),
            ));
        }

        Ok(())
    }

    /// This is called the first time by each peer after connecting.
    pub fn hello(&mut self) -> fdo::Result<OwnedUniqueName> {
        if self.greeted {
            return Err(fdo::Error::Failed(
                "Can only call `Hello` method once".to_string(),
            ));
        }
        self.greeted = true;

        Ok(self.unique_name.clone())
    }
}
