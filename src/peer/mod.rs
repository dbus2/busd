mod stream;
use event_listener::{Event, EventListener};
pub use stream::*;
mod monitor;
pub use monitor::*;

use anyhow::Result;
use tracing::trace;
use zbus::{
    connection, connection::socket::BoxedSplit, names::OwnedUniqueName, AuthMechanism, Connection,
    OwnedGuid, OwnedMatchRule,
};

use crate::{fdo, match_rules::MatchRules, name_registry::NameRegistry};

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    conn: Connection,
    unique_name: OwnedUniqueName,
    match_rules: MatchRules,
    greeted: bool,
    canceled_event: Event,
}

impl Peer {
    pub async fn new(
        guid: OwnedGuid,
        id: usize,
        socket: BoxedSplit,
        auth_mechanism: AuthMechanism,
    ) -> Result<Self> {
        let unique_name = OwnedUniqueName::try_from(format!(":busd.{id}")).unwrap();
        let conn = connection::Builder::socket(socket)
            .server(guid)?
            .p2p()
            .auth_mechanism(auth_mechanism)
            .build()
            .await?;
        trace!("created: {:?}", conn);

        Ok(Self {
            conn,
            unique_name,
            match_rules: MatchRules::default(),
            greeted: false,
            canceled_event: Event::new(),
        })
    }

    // This the the bus itself, serving the FDO D-Bus API.
    pub async fn new_us(conn: Connection) -> Self {
        let unique_name = OwnedUniqueName::try_from(fdo::BUS_NAME).unwrap();

        Self {
            conn,
            unique_name,
            match_rules: MatchRules::default(),
            greeted: true,
            canceled_event: Event::new(),
        }
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

    pub fn listen_cancellation(&self) -> EventListener {
        self.canceled_event.listen()
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

    /// This can only be called once.
    pub async fn hello(&mut self) -> zbus::fdo::Result<()> {
        if self.greeted {
            return Err(zbus::fdo::Error::Failed(
                "Can only call `Hello` method once".to_string(),
            ));
        }
        self.greeted = true;

        Result::Ok(())
    }

    pub fn become_monitor(self, match_rules: MatchRules) -> Monitor {
        Monitor::new(self, match_rules)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.canceled_event.notify(usize::MAX);
    }
}
