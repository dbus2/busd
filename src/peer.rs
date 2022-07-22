use anyhow::Result;
use tracing::trace;
use tokio::net::UnixStream;
use zbus::{dbus_interface, fdo, names::OwnedUniqueName, Connection, ConnectionBuilder, Guid};

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    _conn: Connection,
}

impl Peer {
    pub async fn new(guid: &Guid, unix_stream: UnixStream) -> Result<Self> {
        let conn = ConnectionBuilder::socket(unix_stream)
            .server(guid)
            .p2p()
            .serve_at("/org/freedesktop/DBus", DBus::default())?
            .build()
            .await?;
        trace!("created: {:?}", conn);

        Ok(Self { _conn: conn })
    }
}

#[derive(Debug, Default)]
struct DBus {
    greeted: bool,
}

#[dbus_interface(interface = "org.freedesktop.DBus")]
impl DBus {
    /// Returns the unique name assigned to the connection.
    async fn hello(&mut self) -> fdo::Result<OwnedUniqueName> {
        if self.greeted {
            return Err(fdo::Error::InvalidArgs("".to_string()));
        }
        let name = OwnedUniqueName::try_from(":zbusd.12345").unwrap();
        self.greeted = true;

        Ok(name)
    }
}