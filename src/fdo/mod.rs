mod dbus;
pub use dbus::*;
mod monitoring;
pub use monitoring::*;
use zbus::{message, names::UniqueName};

pub const BUS_NAME: &str = "org.freedesktop.DBus";

/// Helper for getting the peer name from a message header.
fn msg_sender<'h>(hdr: &'h message::Header<'h>) -> &'h UniqueName<'h> {
    // SAFETY: The bus (that's us!) is supposed to ensure a valid sender on the message.
    hdr.sender().expect("Missing `sender` header")
}
