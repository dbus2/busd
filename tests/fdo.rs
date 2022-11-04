use std::{
    iter::repeat_with,
    path::{Path, PathBuf},
};

use anyhow::ensure;
use dbuz::bus::Bus;
use tokio::{select, sync::oneshot::Sender};
use tracing::instrument;
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter, FmtSubscriber};
use zbus::{
    fdo::{DBusProxy, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::WellKnownName,
    CacheProperties, ConnectionBuilder,
};

// TODO: timeout through `ntest`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[instrument]
async fn name_ownership_changes() {
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish()
        .init();

    let s: String = repeat_with(fastrand::alphanumeric).take(10).collect();
    let path = PathBuf::from("/tmp").join(s);

    let mut bus = Bus::new(Some(&*path)).await.unwrap();
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

    let ret = name_ownership_changes_client(&*path, tx).await;
    let bus = handle.await.unwrap();
    bus.cleanup().await.unwrap();
    ret.unwrap();
}

#[instrument]
async fn name_ownership_changes_client(path: &Path, tx: Sender<()>) -> anyhow::Result<()> {
    let socket_addr = format!("unix:path={}", path.display());
    let conn = ConnectionBuilder::address(&*socket_addr)?.build().await?;
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
    let conn2 = ConnectionBuilder::address(&*socket_addr)?.build().await?;
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
