use std::{iter::repeat_with, path::PathBuf};

use tokio::select;
use tracing::instrument;
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter, FmtSubscriber};
use zbus::{
    fdo::{DBusProxy, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    names::WellKnownName,
    CacheProperties, ConnectionBuilder,
};
use zbusd::bus::Bus;

// TODO: timeout through `ntest`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[instrument]
async fn test() {
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

    let socket_addr = format!("unix:path={}", path.display());
    let conn = ConnectionBuilder::address(&*socket_addr)
        .unwrap()
        .build()
        .await
        .unwrap();
    let dbus_proxy = DBusProxy::builder(&conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .unwrap();
    let name: WellKnownName = "org.blah".try_into().unwrap();

    // This should work.
    let ret = dbus_proxy
        .request_name(name.clone(), RequestNameFlags::AllowReplacement.into())
        .await
        .unwrap();
    assert_eq!(ret, RequestNameReply::PrimaryOwner);

    // This shouldn't and we should be told we already own the name.
    let ret = dbus_proxy
        .request_name(name.clone(), RequestNameFlags::AllowReplacement.into())
        .await
        .unwrap();
    assert_eq!(ret, RequestNameReply::AlreadyOwner);

    // Now we try with another connection and we should be queued.
    let conn2 = ConnectionBuilder::address(&*socket_addr)
        .unwrap()
        .build()
        .await
        .unwrap();
    let dbus_proxy2 = DBusProxy::builder(&conn2)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .unwrap();
    let ret = dbus_proxy2
        .request_name(name.clone(), Default::default())
        .await
        .unwrap();

    // Check that first client is the primary owner before it releases the name.
    assert_eq!(ret, RequestNameReply::InQueue);
    let owner = dbus_proxy
        .get_name_owner(name.clone().into())
        .await
        .unwrap();
    assert_eq!(owner, *conn.unique_name().unwrap());

    // Now the first client releases name.
    let ret = dbus_proxy.release_name(name.clone()).await.unwrap();
    assert_eq!(ret, ReleaseNameReply::Released);

    // Now the second client should be the primary owner.
    let owner = dbus_proxy
        .get_name_owner(name.clone().into())
        .await
        .unwrap();
    assert_eq!(owner, *conn2.unique_name().unwrap());

    tx.send(()).unwrap();

    let bus = handle.await.unwrap();
    bus.cleanup().await.unwrap();
}
