use std::{env::temp_dir, iter::repeat_with};

use anyhow::ensure;
use dbuz::bus::Bus;
use ntest::timeout;
use tokio::{select, sync::oneshot::Sender};
use tracing::instrument;
use zbus::{
    fdo::{DBusProxy, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::WellKnownName,
    CacheProperties, ConnectionBuilder,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[instrument]
#[timeout(15000)]
async fn name_ownership_changes() {
    dbuz::tracing_subscriber::init();

    // Unix socket
    let s: String = repeat_with(fastrand::alphanumeric).take(10).collect();
    let path = temp_dir().join(s);
    let address = format!("unix:path={}", path.display());
    name_ownership_changes_(&address, false).await;

    // TCP socket
    let address = format!("tcp:host=127.0.0.1,port=4242");
    name_ownership_changes_(&address, true).await;
}

async fn name_ownership_changes_(address: &str, allow_anonymous: bool) {
    let mut bus = Bus::for_address(Some(address), allow_anonymous)
        .await
        .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();

    let handle = tokio::spawn(async move {
        select! {
            _ = rx => (),
            _ = bus.run() => {
                panic!("Bus stopped unexpectedly");
            }
        }

        bus
    });

    let ret = name_ownership_changes_client(address, tx).await;
    let bus = handle.await.unwrap();
    bus.cleanup().await.unwrap();
    ret.unwrap();
}

#[instrument]
async fn name_ownership_changes_client(address: &str, tx: Sender<()>) -> anyhow::Result<()> {
    let conn = ConnectionBuilder::address(address)?.build().await?;
    let dbus_proxy = DBusProxy::builder(&conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let name: WellKnownName = "org.blah".try_into()?;

    // This should work.
    let ret = dbus_proxy
        .request_name(name.clone(), RequestNameFlags::AllowReplacement.into())
        .await?;
    ensure!(
        ret == RequestNameReply::PrimaryOwner,
        "expected to become primary owner"
    );

    // This shouldn't and we should be told we already own the name.
    let ret = dbus_proxy
        .request_name(name.clone(), RequestNameFlags::AllowReplacement.into())
        .await?;
    ensure!(
        ret == RequestNameReply::AlreadyOwner,
        "expected to be already primary owner"
    );

    // Now we try with another connection and we should be queued.
    let conn2 = ConnectionBuilder::address(address)?.build().await?;
    let dbus_proxy2 = DBusProxy::builder(&conn2)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let ret = dbus_proxy2
        .request_name(name.clone(), Default::default())
        .await?;

    // Check that first client is the primary owner before it releases the name.
    ensure!(ret == RequestNameReply::InQueue, "expected to be in queue");
    let owner = dbus_proxy.get_name_owner(name.clone().into()).await?;
    ensure!(owner == *conn.unique_name().unwrap(), "unexpected owner");

    // Now the first client releases name.
    let ret = dbus_proxy.release_name(name.clone()).await?;
    ensure!(
        ret == ReleaseNameReply::Released,
        "expected name to be released"
    );

    // Now the second client should be the primary owner.
    let owner = dbus_proxy.get_name_owner(name.clone().into()).await?;
    ensure!(owner == *conn2.unique_name().unwrap(), "unexpected owner");

    tx.send(()).unwrap();

    Ok(())
}
