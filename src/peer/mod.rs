mod stream;
pub use stream::*;

use std::sync::Arc;

use anyhow::Result;
use tracing::trace;
use zbus::{
    names::OwnedUniqueName, AuthMechanism, Connection, ConnectionBuilder, Guid, OwnedMatchRule,
    Socket,
};

use crate::{
    fdo::{self, DBus},
    match_rules::MatchRules,
    name_registry::NameRegistry,
    peers::Peers,
};

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    conn: Connection,
    unique_name: OwnedUniqueName,
    match_rules: MatchRules,
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
            match_rules: MatchRules::default(),
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
    /// Same as [`MatchRules::matches`].
    pub fn interested(&self, msg: &zbus::Message, name_registry: &NameRegistry) -> bool {
        self.match_rules.matches(msg, name_registry)
    }

    pub fn add_match_rule(&mut self, rule: OwnedMatchRule) {
        self.match_rules.add(rule);
    }

    /// Remove the first rule that matches.
    pub fn remove_match_rule(&mut self, rule: OwnedMatchRule) -> zbus::fdo::Result<()> {
        self.match_rules.remove(rule)
    }
}
