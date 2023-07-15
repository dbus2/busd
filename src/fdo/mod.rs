mod dbus;
pub use dbus::*;
use zbus::{names::UniqueName, MessageHeader};

pub const BUS_NAME: &str = "org.freedesktop.DBus";

/// Helper for getting the peer name from a message header.
fn msg_sender<'h>(hdr: &'h MessageHeader<'h>) -> &'h UniqueName<'h> {
    // SAFETY: The bus (that's us!) is supposed to ensure a valid sender on the message.
    hdr.sender()
        .ok()
        .flatten()
        .expect("Missing `sender` header")
}
