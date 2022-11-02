use std::sync::Arc;

use anyhow::Result;
use enumflags2::BitFlags;
use tokio::net::UnixStream;
use tracing::trace;
use zbus::{
    dbus_interface,
    fdo::{self, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::{BusName, OwnedBusName, OwnedUniqueName, OwnedWellKnownName},
    Connection, ConnectionBuilder, Guid, MessageStream,
};

use crate::name_registry::NameRegistry;

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    conn: Connection,
    unique_name: Arc<OwnedUniqueName>,
}

impl Peer {
    pub async fn new(
        guid: &Guid,
        id: usize,
        unix_stream: UnixStream,
        name_registry: NameRegistry,
    ) -> Result<Self> {
        let unique_name = Arc::new(OwnedUniqueName::try_from(format!(":zbusd.{}", id)).unwrap());

        let conn = ConnectionBuilder::socket(unix_stream)
            .server(guid)
            .p2p()
            .serve_at(
                "/org/freedesktop/DBus",
                DBus::new(unique_name.clone(), name_registry),
            )?
            .name("org.freedesktop.DBus")?
            .build()
            .await?;
        conn.set_unique_name("org.freedesktop.DBus")?;
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
}

#[derive(Debug)]
struct DBus {
    greeted: bool,
    unique_name: Arc<OwnedUniqueName>,
    name_registry: NameRegistry,
}

impl DBus {
    fn new(unique_name: Arc<OwnedUniqueName>, name_registry: NameRegistry) -> Self {
        Self {
            greeted: false,
            unique_name,
            name_registry,
        }
    }
}

#[dbus_interface(interface = "org.freedesktop.DBus")]
impl DBus {
    /// Returns the unique name assigned to the connection.
    async fn hello(&mut self) -> fdo::Result<Arc<OwnedUniqueName>> {
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
    fn get_name_owner(&self, name: OwnedBusName) -> fdo::Result<Arc<OwnedUniqueName>> {
        match name.into_inner() {
            BusName::WellKnown(name) => self.name_registry.lookup(name).ok_or_else(|| {
                fdo::Error::NameHasNoOwner("Name is not owned by anyone. Take it!".to_string())
            }),
            // FIXME: Not good enough. We need to check if name is actually owned.
            BusName::Unique(name) => Ok(Arc::new(name.into())),
        }
    }
}
