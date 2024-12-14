#[cfg(unix)]
use std::{env, path::PathBuf};

use anyhow::{Error, Result};
use zbus::Address;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BusType {
    #[default]
    Session,
    System,
}

impl TryFrom<BusType> for Address {
    type Error = Error;
    #[cfg(unix)]
    fn try_from(value: BusType) -> Result<Self> {
        if value == BusType::System {
            return Address::try_from("unix:path=/run/dbus/system_bus_socket").map_err(Error::msg);
        }

        // BusType::Session
        Address::try_from(format!("unix:tmpdir={}", default_session_dir().display()).as_str())
            .map_err(Error::msg)
    }

    #[cfg(not(unix))]
    fn try_from(_value: BusType) -> Result<Self> {
        Address::try_from("tcp:host=127.0.0.1,port=4242").map_err(Error::msg)
    }
}

#[cfg(unix)]
fn default_session_dir() -> PathBuf {
    env::var("XDG_RUNTIME_DIR")
        .map(|s| PathBuf::from(s).to_path_buf())
        .ok()
        .unwrap_or_else(|| {
            PathBuf::from("/run")
                .join("user")
                .join(format!("{}", nix::unistd::Uid::current()))
        })
}
