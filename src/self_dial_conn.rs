use anyhow::Result;
use futures_util::{try_join, TryFutureExt};
use tracing::trace;
use zbus::{Address, Connection, ConnectionBuilder};

use crate::{bus::Bus, fdo::DBus, peer::Peer};

#[derive(Debug)]
pub struct SelfDialConn {
    _conn: Connection,
}

impl SelfDialConn {
    pub async fn connect(bus: &mut Bus, address: Address) -> Result<(Self, Peer)> {
        // Create a peer for ourselves.
        let dbus = DBus::new(bus.peers().clone(), bus.guid().clone());
        trace!("Creating self-dial connection.");
        let conn_builder_fut = ConnectionBuilder::address(address)?
            .serve_at("/org/freedesktop/DBus", dbus)?
            .auth_mechanisms(&[bus.auth_mechanism()])
            .p2p()
            .unique_name("org.freedesktop.DBus")?
            .build()
            .map_err(Into::into);

        let (conn, self_dial_peer) = try_join!(conn_builder_fut, bus.accept_next())?;
        trace!("Self-dial connection created.");

        Ok((Self { _conn: conn }, self_dial_peer))
    }
}
