use std::{env::temp_dir, time::Duration};

use anyhow::anyhow;
use busd::bus::Bus;
use futures_util::{pin_mut, stream::StreamExt};
use ntest::timeout;
use rand::{
    distributions::{Alphanumeric, DistString},
    thread_rng,
};
use tokio::{select, sync::mpsc::channel, time::timeout};
use tracing::instrument;
use zbus::{
    dbus_interface, dbus_proxy,
    fdo::{self, DBusProxy},
    zvariant::ObjectPath,
    AsyncDrop, AuthMechanism, CacheProperties, Connection, ConnectionBuilder, MatchRule,
    MessageHeader, MessageStream, SignalContext,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[instrument]
#[timeout(15000)]
async fn greet() {
    busd::tracing_subscriber::init();

    // Unix socket
    #[cfg(unix)]
    {
        let s = Alphanumeric.sample_string(&mut thread_rng(), 10);
        let path = temp_dir().join(s);
        let address = format!("unix:path={}", path.display());
        greet_(&address, AuthMechanism::External).await;
    }

    // TCP socket
    let address = format!("tcp:host=127.0.0.1,port=4248");
    greet_(&address, AuthMechanism::Cookie).await;
    greet_(&address, AuthMechanism::Anonymous).await;
}

async fn greet_(socket_addr: &str, auth_mechanism: AuthMechanism) {
    let mut bus = Bus::for_address(socket_addr, auth_mechanism).await.unwrap();
    let (tx, mut rx) = channel(1);

    let handle = tokio::spawn(async move {
        select! {
            _ = rx.recv() => (),
            res = bus.run() => match res {
                Ok(()) => panic!("Bus exited unexpectedly"),
                Err(e) => panic!("Bus exited with an error: {}", e),
            }
        }

        bus
    });

    let ret = match greet_service(socket_addr).await {
        Ok(service_conn) => greet_client(socket_addr).await.map(|_| service_conn),
        Err(e) => Err(e),
    };
    let _ = tx.send(()).await;
    let bus = handle.await.unwrap();
    bus.cleanup().await.unwrap();
    let _ = ret.unwrap();
}

#[instrument]
async fn greet_service(socket_addr: &str) -> anyhow::Result<Connection> {
    struct Greeter {
        count: u64,
    }

    #[dbus_interface(name = "org.zbus.MyGreeter1")]
    impl Greeter {
        async fn say_hello(
            &mut self,
            name: &str,
            #[zbus(signal_context)] ctxt: SignalContext<'_>,
            #[zbus(header)] header: MessageHeader<'_>,
        ) -> fdo::Result<String> {
            self.count += 1;
            let path = header.path()?.unwrap().clone();
            Self::greeted(&ctxt, name, self.count, path).await?;
            Ok(format!(
                "Hello {}! I have been called {} times.",
                name, self.count
            ))
        }

        #[dbus_interface(signal)]
        async fn greeted(
            ctxt: &SignalContext<'_>,
            name: &str,
            count: u64,
            path: ObjectPath<'_>,
        ) -> zbus::Result<()>;
    }

    let greeter = Greeter { count: 0 };
    ConnectionBuilder::address(socket_addr)?
        .name("org.zbus.MyGreeter")?
        .serve_at("/org/zbus/MyGreeter", greeter)?
        .build()
        .await
        .map_err(Into::into)
}

#[instrument]
async fn greet_client(socket_addr: &str) -> anyhow::Result<()> {
    #[dbus_proxy(
        interface = "org.zbus.MyGreeter1",
        default_path = "/org/zbus/MyGreeter"
    )]
    trait MyGreeter {
        fn say_hello(&self, name: &str) -> zbus::Result<String>;

        #[dbus_proxy(signal)]
        async fn greeted(name: &str, count: u64, path: ObjectPath<'_>);
    }

    let conn = ConnectionBuilder::address(socket_addr)?.build().await?;

    let proxy = MyGreeterProxy::builder(&conn)
        .destination("org.zbus.MyGreeter")?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let mut greeted_stream = proxy.receive_greeted().await?;
    let reply = proxy.say_hello("Maria").await?;
    assert_eq!(reply, "Hello Maria! I have been called 1 times.");
    let signal = greeted_stream
        .next()
        .await
        .ok_or(anyhow!("stream ended unexpectedly"))?;
    let args = signal.args()?;
    assert_eq!(args.name, "Maria");
    assert_eq!(args.count, 1);
    assert_eq!(args.path, "/org/zbus/MyGreeter");

    // Now let's unsubcribe from the signal and ensure we don't receive it anymore.
    greeted_stream.async_drop().await;
    let msg_stream = MessageStream::from(&conn).filter_map(|msg| async {
        let msg = msg.ok()?;
        Greeted::from_message(msg)
    });
    pin_mut!(msg_stream);
    let _ = proxy.say_hello("Maria").await?;
    timeout(Duration::from_millis(10), msg_stream.next())
        .await
        .unwrap_err();

    // Now let's try a manual subscription.
    let match_rule = MatchRule::builder()
        .interface("org.zbus.MyGreeter1")?
        .member("Greeted")?
        .add_arg("Maria")?
        .arg_path(2, "/org/zbus/MyGreeter")?
        .build();
    DBusProxy::new(&conn)
        .await?
        .add_match_rule(match_rule)
        .await?;
    let _ = proxy.say_hello("Maria").await?;
    let signal = msg_stream.next().await.unwrap();
    let args = signal.args()?;
    assert_eq!(args.name, "Maria");

    Ok(())
}
