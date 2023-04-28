mod fdo;

use anyhow::Result;
use tracing::trace;
use zbus::{
    names::{BusName, OwnedUniqueName},
    AuthMechanism, Connection, ConnectionBuilder, Guid, MessageStream, Socket,
};

use crate::name_registry::NameRegistry;
use fdo::DBus;

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    conn: Connection,
    name_registry: NameRegistry,
    unique_name: OwnedUniqueName,
}

impl Peer {
    pub async fn new(
        guid: &Guid,
        id: usize,
        socket: Box<dyn Socket + 'static>,
        name_registry: NameRegistry,
        auth_mechanism: AuthMechanism,
    ) -> Result<Self> {
        let unique_name = OwnedUniqueName::try_from(format!(":busd.{id}")).unwrap();

        let conn = ConnectionBuilder::socket(socket)
            .server(guid)
            .p2p()
            .serve_at(
                "/org/freedesktop/DBus",
                DBus::new(unique_name.clone(), name_registry.clone()),
            )?
            .name("org.freedesktop.DBus")?
            .unique_name("org.freedesktop.DBus")?
            .auth_mechanisms(&[auth_mechanism])
            .build()
            .await?;
        trace!("created: {:?}", conn);

        Ok(Self {
            conn,
            name_registry,
            unique_name,
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
        let dbus_ref = self
            .conn
            .object_server()
            .interface::<_, DBus>("/org/freedesktop/DBus")
            .await
            .expect("DBus interface not found");
        let dbus = dbus_ref.get().await;
        let hdr = msg.header().expect("received message without header");

        let ret = dbus.match_rules().any(|rule| {
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
}
