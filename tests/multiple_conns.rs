use std::env::temp_dir;

use busd::bus::Bus;
use futures_util::future::join_all;
use ntest::timeout;
use rand::{
    distributions::{Alphanumeric, DistString},
    thread_rng,
};
use tokio::{select, sync::oneshot::channel};
use tracing::instrument;
use zbus::{AuthMechanism, ConnectionBuilder};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[instrument]
#[timeout(15000)]
async fn multi_conenct() {
    busd::tracing_subscriber::init();

    #[cfg(unix)]
    {
        let s = Alphanumeric.sample_string(&mut thread_rng(), 10);
        let path = temp_dir().join(s);
        let address = format!("unix:path={}", path.display());
        multi_conenct_(&address, AuthMechanism::External).await;
    }

    // TCP socket
    let address = format!("tcp:host=127.0.0.1,port=4246");
    multi_conenct_(&address, AuthMechanism::Cookie).await;
    multi_conenct_(&address, AuthMechanism::Anonymous).await;
}

async fn multi_conenct_(socket_addr: &str, auth_mechanism: AuthMechanism) {
    let mut bus = Bus::for_address(socket_addr, auth_mechanism).await.unwrap();
    let (tx, rx) = channel();

    let handle = tokio::spawn(async move {
        select! {
            _ = rx => (),
            res = bus.run() => match res {
                Ok(()) => panic!("Bus exited unexpectedly"),
                Err(e) => panic!("Bus exited with an error: {}", e),
            }
        }

        bus
    });

    let ret = multi_clients_connect(socket_addr).await;
    let _ = tx.send(());
    let bus = handle.await.unwrap();
    bus.cleanup().await.unwrap();
    ret.unwrap();
}

#[instrument]
async fn multi_clients_connect(socket_addr: &str) -> anyhow::Result<()> {
    // Create 10 connections simultaneously.
    let conns: Vec<_> = (0..10)
        .map(|_| ConnectionBuilder::address(socket_addr).unwrap().build())
        .collect();
    join_all(conns).await;

    Ok(())
}
