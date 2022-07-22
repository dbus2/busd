use std::env;

use clap::Parser;
use nix::unistd::Uid;
use tracing::{debug, trace};
use tracing_subscriber::{util::SubscriberInitExt, EnvFilter, FmtSubscriber};
use zbus::{dbus_interface, fdo, names::OwnedUniqueName, ConnectionBuilder, Guid};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The socket path.
    #[clap(short = 's', long, value_parser)]
    socket_path: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish()
        .init();

    let args = Args::parse();

    let runtime_dir = args
        .socket_path
        .or_else(|| env::var("XDG_RUNTIME_DIR").ok())
        .unwrap_or_else(|| format!("/run/user/{}", Uid::current()));
    let path = format!("{}/zbusd-session", runtime_dir);
    let listener = tokio::net::UnixListener::bind(&path)?;
    let mut connections = vec![];

    while let Ok((unix_stream, addr)) = listener.accept().await {
        debug!("Accepted connection from {:?}", addr);
        let guid = Guid::generate();
        let conn = ConnectionBuilder::socket(unix_stream)
            .server(&guid)
            .p2p()
            .serve_at("/org/freedesktop/DBus", DBus::default())?
            .build()
            .await?;
        trace!("created: {:?}", conn);
        connections.push(conn);
    }

    Ok(())
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
