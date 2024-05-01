extern crate busd;

#[cfg(unix)]
use std::{fs::File, io::Write, os::fd::FromRawFd};

use busd::bus;

use anyhow::Result;
use clap::{Parser, ValueEnum};
#[cfg(unix)]
use tokio::{select, signal::unix::SignalKind};
use tracing::error;
#[cfg(unix)]
use tracing::{info, warn};

/// A simple D-Bus broker.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The address to listen on.
    #[clap(short = 'a', long, value_parser)]
    address: Option<String>,

    /// Print the address of the message bus to standard output.
    #[clap(long)]
    print_address: bool,

    /// The authentication mechanism to use.
    #[clap(long)]
    #[arg(value_enum, default_value_t = AuthMechanism::External)]
    auth_mechanism: AuthMechanism,

    /// File descriptor to which readiness notifications are sent.
    ///
    /// Once the server is listening to connections on the specified socket, it will print
    /// `READY=1\n` into this file descriptor and close it.
    ///
    /// This readiness notification mechanism which works on both systemd and s6.
    ///
    /// This feature is only available on unix-like platforms.
    #[cfg(unix)]
    #[clap(long)]
    ready_fd: Option<i32>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum AuthMechanism {
    /// This is the recommended authentication mechanism on platforms where credentials can be
    /// transferred out-of-band, in particular Unix platforms that can perform credentials-passing
    /// over UNIX domain sockets.
    External,
    /// This mechanism is designed to establish that a client has the ability to read a private
    /// file owned by the user being authenticated.
    Cookie,
    /// Does not perform any authentication at all (not recommended).
    Anonymous,
}

impl From<AuthMechanism> for zbus::AuthMechanism {
    fn from(auth_mechanism: AuthMechanism) -> Self {
        match auth_mechanism {
            AuthMechanism::External => zbus::AuthMechanism::External,
            AuthMechanism::Cookie => zbus::AuthMechanism::Cookie,
            AuthMechanism::Anonymous => zbus::AuthMechanism::Anonymous,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    busd::tracing_subscriber::init();

    let args = Args::parse();

    let mut bus =
        bus::Bus::for_address(args.address.as_deref(), args.auth_mechanism.into()).await?;

    #[cfg(unix)]
    if let Some(fd) = args.ready_fd {
        // SAFETY: We don't have any way to know if the fd is valid or not. The parent process is
        // responsible for passing a valid fd.
        let mut ready_file = unsafe { File::from_raw_fd(fd) };
        ready_file.write_all(b"READY=1\n")?;
    }

    if args.print_address {
        println!("{}", bus.address());
    }

    // FIXME: How to handle this gracefully on Windows?
    #[cfg(unix)]
    {
        let mut sig_int = tokio::signal::unix::signal(SignalKind::interrupt())?;

        select! {
            _ = sig_int.recv() => {
                info!("Received SIGINT, shutting down..");
            }
            res = bus.run() => match res {
                Ok(()) => warn!("Bus stopped, shutting down.."),
                Err(e) => error!("Bus stopped with an error: {}", e),
            }
        }
    }
    #[cfg(not(unix))]
    bus.run().await?;

    if let Err(e) = bus.cleanup().await {
        error!("Failed to clean up: {}", e);
    }

    Ok(())
}
