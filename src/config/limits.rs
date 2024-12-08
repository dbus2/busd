use std::time::Duration;

use super::BusType;

#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// total size in bytes of messages incoming from a single connection
    pub max_incoming_bytes: u32,
    /// total number of unix fds of messages incoming from a single connection
    pub max_incoming_unix_fds: u32,
    /// total size in bytes of messages queued up for a single connection
    pub max_outgoing_bytes: u32,
    /// total number of unix fds of messages queued up for a single connection
    pub max_outgoing_unix_fds: u32,
    /// max size of a single message in bytes
    pub max_message_size: u32,
    /// max unix fds of a single message
    pub max_message_unix_fds: u32,
    /// time a started service has to connect
    pub service_start_timeout: Duration,
    /// time a connection is given to authenticate
    pub auth_timeout: Duration,
    /// time a fd is given to be transmitted to dbus-daemon before disconnecting the connection
    pub pending_fd_timeout: Duration,
    /// max number of authenticated connections
    pub max_completed_connections: u32,
    /// max number of unauthenticated connections
    pub max_incomplete_connections: u32,
    /// max number of completed connections from the same user (only enforced on Unix OSs)
    pub max_connections_per_user: u32,
    /// max number of service launches in progress at the same time
    pub max_pending_service_starts: u32,
    /// max number of names a single connection can own
    pub max_names_per_connection: u32,
    /// max number of match rules for a single connection
    pub max_match_rules_per_connection: u32,
    /// max number of pending method replies per connection (number of calls-in-progress)
    pub max_replies_per_connection: u32,
    /// time until a method call times out
    pub reply_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_incoming_bytes: 133169152,
            max_incoming_unix_fds: 64,
            max_outgoing_bytes: 133169152,
            max_outgoing_unix_fds: 64,
            max_message_size: 33554432,
            max_message_unix_fds: 16,
            service_start_timeout: Duration::from_millis(25000),
            auth_timeout: Duration::from_millis(5000),
            pending_fd_timeout: Duration::from_millis(150000),
            max_completed_connections: 2048,
            max_incomplete_connections: 64,
            max_connections_per_user: 256,
            max_pending_service_starts: 512,
            max_names_per_connection: 512,
            max_match_rules_per_connection: 512,
            max_replies_per_connection: 128,
            reply_timeout: Duration::from_millis(5000),
        }
    }
}
impl From<BusType> for Limits {
    fn from(value: BusType) -> Self {
        if value == BusType::Session {
            Self {
                // dbus-daemon / dbus-broker is limited to the highest positive number in i32,
                // but we use u32 here, so we can pick the preferred 4GB memory limits
                max_incoming_bytes: 4000000000,
                max_incoming_unix_fds: 250000000,
                max_outgoing_bytes: 4000000000,
                max_outgoing_unix_fds: 250000000,
                max_message_size: 4000000000,
                // We do not override max_message_unix_fds here,
                // since the in-kernel limit is also relatively low
                max_message_unix_fds: 16,
                service_start_timeout: Duration::from_millis(120000),
                auth_timeout: Duration::from_millis(240000),
                pending_fd_timeout: Duration::from_millis(150000),
                max_completed_connections: 100000,
                max_incomplete_connections: 10000,
                max_connections_per_user: 100000,
                max_pending_service_starts: 10000,
                max_names_per_connection: 50000,
                max_match_rules_per_connection: 50000,
                max_replies_per_connection: 50000,
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
}
