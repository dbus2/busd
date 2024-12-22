#[cfg(unix)]
use std::{env, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BusType {
    #[default]
    Session,
    System,
}

impl BusType {
    #[cfg(unix)]
    pub fn default_address(&self) -> String {
        if self == &BusType::System {
            return String::from("unix:path=/run/dbus/system_bus_socket");
        }

        // BusType::Session
        format!("unix:tmpdir={}", default_session_dir().display())
    }

    #[cfg(not(unix))]
    pub fn default_address(&self) -> String {
        String::from("tcp:host=127.0.0.1,port=4242")
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
