use std::sync::Arc;

use anyhow::Result;
use tokio::net::UnixStream;
use tracing::trace;
use zbus::{dbus_interface, fdo, names::OwnedUniqueName, Connection, ConnectionBuilder, Guid};

/// A peer connection.
#[derive(Debug)]
pub struct Peer {
    _conn: Connection,
    unique_name: Arc<OwnedUniqueName>,
}

impl Peer {
    pub async fn new(guid: &Guid, id: usize, unix_stream: UnixStream) -> Result<Self> {
        let unique_name = Arc::new(OwnedUniqueName::try_from(format!(":zbusd.{}", id)).unwrap());

        let conn = ConnectionBuilder::socket(unix_stream)
            .server(guid)
            .p2p()
            .serve_at("/org/freedesktop/DBus", DBus::new(unique_name.clone()))?
            .name("org.freedesktop.DBus")?
            .build()
            .await?;
        conn.set_unique_name("org.freedesktop.DBus")?;
        trace!("created: {:?}", conn);

        Ok(Self {
            _conn: conn,
            unique_name,
        })
    }

    pub fn unique_name(&self) -> &OwnedUniqueName {
        &self.unique_name
    }
}

#[derive(Debug)]
struct DBus {
    greeted: bool,
    unique_name: Arc<OwnedUniqueName>,
}

impl DBus {
    fn new(unique_name: Arc<OwnedUniqueName>) -> Self {
        Self {
            greeted: false,
            unique_name,
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
}
