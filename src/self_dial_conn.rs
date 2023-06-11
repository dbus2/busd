use anyhow::Result;
use futures_util::{try_join, TryFutureExt};
use tokio::sync::mpsc::Receiver;
use tracing::trace;
use zbus::{Address, Connection, ConnectionBuilder};

use crate::{bus::Bus, fdo::DBus, name_registry::NameOwnerChanged, peer::Peer};

#[derive(Debug)]
pub struct SelfDialConn {
    conn: Connection,
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
        let peer_setup_fut = async {
            let socket = bus.accept().await?;

            Peer::new(
                bus.guid(),
                None,
                socket,
                bus.auth_mechanism(),
                bus.peers().clone(),
            )
            .await
        };

        let (conn, self_dial_peer) = try_join!(conn_builder_fut, peer_setup_fut)?;
        trace!("Self-dial connection created.");

        Ok((Self { conn }, self_dial_peer))
    }

    pub async fn monitor_name_changes(
        self,
        mut name_changed_rx: Receiver<NameOwnerChanged>,
    ) -> Result<()> {
        while let Some(name_changed) = name_changed_rx.recv().await {
            trace!("Name changed: {:?}", name_changed);
            let dbus = self
                .conn
                .object_server()
                .interface::<_, DBus>("/org/freedesktop/DBus")
                .await?;

            // First broadcast the name change signal.
            let ctxt = dbus.signal_context();
            let old_owner = name_changed.old_owner.map(Into::into);
            let new_owner = name_changed.new_owner.map(Into::into);
            DBus::name_owner_changed(
                ctxt,
                name_changed.name.clone().into(),
                old_owner.clone().into(),
                new_owner.clone().into(),
            )
            .await?;

            // Now unicast the appropriate signal to the old and new owners.
            if let Some(old_owner) = old_owner {
                let ctxt = ctxt.clone().set_destination(old_owner.into());
                DBus::name_lost(&ctxt, name_changed.name.clone().into()).await?;
            }
            if let Some(new_owner) = new_owner {
                let ctxt = ctxt.clone().set_destination(new_owner.into());
                DBus::name_acquired(&ctxt, name_changed.name.into()).await?;
            }
        }

        Ok(())
    }
}
