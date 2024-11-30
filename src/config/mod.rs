use std::{
    env::var,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Error, Result};
use serde::Deserialize;
use zbus::{Address, AuthMechanism};

mod xml;

use xml::{
    Document, Element, PolicyContext, PolicyElement, RuleAttributes, RuleElement, TypeElement,
};

/// The bus configuration.
///
/// This is currently only loaded from the [XML configuration files] defined by the specification.
/// We plan to add support for other formats (e.g JSON) in the future.
///
/// [XML configuration files]: https://dbus.freedesktop.org/doc/dbus-daemon.1.html#configuration_file
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Config {
    /// If `true`, connections that authenticated using the ANONYMOUS mechanism will be authorized
    /// to connect. This option has no practical effect unless the ANONYMOUS mechanism has also
    /// been enabled using the `auth` option.
    pub allow_anonymous: bool,

    /// Lists permitted authorization mechanisms.
    /// If this element doesn't exist, then all known mechanisms are allowed.
    // TODO: warn when multiple `<auth>` elements are defined, as we only support one
    // TODO: consider implementing `Deserialize` over in zbus crate, then removing this "skip..."
    #[serde(default, skip_deserializing)]
    pub auth: Option<AuthMechanism>,

    /// If `true`, the bus daemon becomes a real daemon (forks into the background, etc.).
    pub fork: bool,

    /// If `true`, the bus daemon keeps its original umask when forking.
    /// This may be useful to avoid affecting the behavior of child processes.
    pub keep_umask: bool,

    /// Address that the bus should listen on.
    /// The address is in the standard D-Bus format that contains a transport name plus possible
    /// parameters/options.
    // TODO: warn when multiple `<listen>` elements are defined, as we only support one
    // TODO: consider implementing `Deserialize` over in zbus crate, then removing this "skip..."
    #[serde(default, skip_deserializing)]
    pub listen: Option<Address>,

    /// The bus daemon will write its pid to the specified file.
    pub pidfile: Option<PathBuf>,

    pub policies: Vec<Policy>,

    /// Adds a directory to search for .service files,
    /// which tell the dbus-daemon how to start a program to provide a particular well-known bus
    /// name.
    #[serde(default)]
    pub servicedirs: Vec<PathBuf>,

    /// Specifies the setuid helper that is used to launch system daemons with an alternate user.
    pub servicehelper: Option<PathBuf>,

    /// If `true`, the bus daemon will log to syslog.
    pub syslog: bool,

    /// This element only controls which message bus specific environment variables are set in
    /// activated clients.
    pub r#type: Option<BusType>,

    /// The user account the daemon should run as, as either a username or a UID.
    /// If the daemon cannot change to this UID on startup, it will exit.
    /// If this element is not present, the daemon will not change or care about its UID.
    pub user: Option<String>,
}

impl TryFrom<Document> for Config {
    type Error = Error;

    fn try_from(value: Document) -> std::result::Result<Self, Self::Error> {
        let mut config = Config::default();

        for element in value.busconfig {
            match element {
                Element::AllowAnonymous => config.allow_anonymous = true,
                Element::Auth(auth) => {
                    config.auth = Some(AuthMechanism::from_str(&auth)?);
                }
                Element::Fork => config.fork = true,
                Element::Include(_) => {
                    // NO-OP: removed during `Document::resolve_includes`
                }
                Element::Includedir(_) => {
                    // NO-OP: removed during `Document::resolve_includedirs`
                }
                Element::KeepUmask => config.keep_umask = true,
                Element::Limit => {
                    // NO-OP: deprecated and ignored
                }
                Element::Listen(listen) => {
                    config.listen = Some(Address::from_str(&listen)?);
                }
                Element::Pidfile(p) => config.pidfile = Some(p),
                Element::Policy(pe) => {
                    if let Some(p) = OptionalPolicy::try_from(pe)? {
                        config.policies.push(p);
                    }
                }
                Element::Servicedir(p) => {
                    config.servicedirs.push(p);
                }
                Element::Servicehelper(p) => {
                    // NOTE: we're assuming this has the same "last one wins" behaviour as `<type>`

                    // TODO: warn and then ignore if we aren't reading:
                    // /usr/share/dbus-1/system.conf
                    config.servicehelper = Some(p);
                }
                Element::StandardSessionServicedirs => {
                    // TODO: warn and then ignore if we aren't reading: /etc/dbus-1/session.conf
                    if let Ok(runtime_dir) = var("XDG_RUNTIME_DIR") {
                        config
                            .servicedirs
                            .push(PathBuf::from(runtime_dir).join("dbus-1/services"));
                    }
                    if let Ok(data_dir) = var("XDG_DATA_HOME") {
                        config
                            .servicedirs
                            .push(PathBuf::from(data_dir).join("dbus-1/services"));
                    }
                    let mut servicedirs_in_data_dirs = xdg_data_dirs()
                        .iter()
                        .map(|p| p.join("dbus-1/services"))
                        .map(PathBuf::from)
                        .collect();
                    config.servicedirs.append(&mut servicedirs_in_data_dirs);
                    config
                        .servicedirs
                        .push(PathBuf::from("/usr/share/dbus-1/services"));
                    // TODO: add Windows-specific session directories
                }
                Element::StandardSystemServicedirs => {
                    // TODO: warn and then ignore if we aren't reading:
                    // /usr/share/dbus-1/system.conf
                    config
                        .servicedirs
                        .extend(STANDARD_SYSTEM_SERVICEDIRS.iter().map(PathBuf::from));
                }
                Element::Syslog => config.syslog = true,
                Element::Type(TypeElement { r#type: value }) => config.r#type = Some(value),
                Element::User(s) => config.user = Some(s),
            }
        }

        Ok(config)
    }
}

impl Config {
    pub fn parse(s: &str) -> Result<Self> {
        // TODO: validate that our DOCTYPE and root element are correct
        quick_xml::de::from_str::<Document>(s)?.try_into()
    }

    pub fn read_file(file_path: impl AsRef<Path>) -> Result<Self> {
        // TODO: error message should contain file path to missing `<include>`
        Document::read_file(&file_path)?.try_into()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ConnectOperation {
    pub group: Option<String>,
    pub user: Option<String>,
}

impl From<RuleAttributes> for ConnectOperation {
    fn from(value: RuleAttributes) -> Self {
        Self {
            group: value.group,
            user: value.user,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BusType {
    Session,
    System,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    #[default]
    #[serde(rename = "*")]
    Any,
    MethodCall,
    MethodReturn,
    Signal,
    Error,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Name {
    #[serde(rename = "*")]
    Any,
    Exact(String),
    Prefix(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum Operation {
    /// rules checked when a new connection to the message bus is established
    Connect(ConnectOperation),
    /// rules checked when a connection attempts to own a well-known bus names
    Own(NameOwnership),
    /// rules that are checked for each recipient of a message
    Receive(ReceiveOperation),
    /// rules that are checked when a connection attempts to send a message
    Send(SendOperation),
}

type OptionalOperation = Option<Operation>;

impl TryFrom<RuleAttributes> for OptionalOperation {
    type Error = Error;

    fn try_from(value: RuleAttributes) -> std::result::Result<Self, Self::Error> {
        let has_connect = value.group.is_some() || value.user.is_some();
        let has_own = value.own.is_some() || value.own_prefix.is_some();
        let has_send = value.send_broadcast.is_some()
            || value.send_destination.is_some()
            || value.send_destination_prefix.is_some()
            || value.send_error.is_some()
            || value.send_interface.is_some()
            || value.send_member.is_some()
            || value.send_path.is_some()
            || value.send_requested_reply.is_some()
            || value.send_type.is_some();
        let has_receive = value.receive_error.is_some()
            || value.receive_interface.is_some()
            || value.receive_member.is_some()
            || value.receive_path.is_some()
            || value.receive_sender.is_some()
            || value.receive_requested_reply.is_some()
            || value.receive_type.is_some();

        let operations_count: i8 = vec![has_connect, has_own, has_receive, has_send]
            .into_iter()
            .map(i8::from)
            .sum();

        if operations_count > 1 {
            return Err(Error::msg(format!("do not mix rule attributes for connect, own, receive, and/or send attributes in the same rule: {value:?}")));
        }

        if has_connect {
            Ok(Some(Operation::Connect(ConnectOperation::from(value))))
        } else if has_own {
            Ok(Some(Operation::Own(NameOwnership::from(value))))
        } else if has_receive {
            Ok(Some(Operation::Receive(ReceiveOperation::from(value))))
        } else if has_send {
            Ok(Some(Operation::Send(SendOperation::from(value))))
        } else {
            Err(Error::msg(format!("rule must specify supported attributes for connect, own, receive, or send operations: {value:?}")))
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct NameOwnership {
    pub own: Option<Name>,
}

impl From<RuleAttributes> for NameOwnership {
    fn from(value: RuleAttributes) -> Self {
        let own = match value {
            RuleAttributes {
                own: Some(some),
                own_prefix: None,
                ..
            } if some == "*" => Some(Name::Any),
            RuleAttributes {
                own: Some(some),
                own_prefix: None,
                ..
            } => Some(Name::Exact(some)),
            RuleAttributes {
                own: None,
                own_prefix: Some(some),
                ..
            } => Some(Name::Prefix(some)),
            _ => None,
        };
        Self { own }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum Policy {
    DefaultContext(Vec<Rule>),
    Group(Vec<Rule>, String),
    MandatoryContext(Vec<Rule>),
    User(Vec<Rule>, String),
}
// TODO: implement Cmp/Ord to help stable-sort Policy values:
// DefaultContext < Group < User < MandatoryContext

type OptionalPolicy = Option<Policy>;

impl TryFrom<PolicyElement> for OptionalPolicy {
    type Error = Error;

    fn try_from(value: PolicyElement) -> std::result::Result<Self, Self::Error> {
        match value {
            PolicyElement {
                at_console: Some(_),
                context: None,
                group: None,
                user: None,
                ..
            } => Ok(None),
            PolicyElement {
                at_console: None,
                context: Some(c),
                group: None,
                rules,
                user: None,
            } => Ok(Some(match c {
                PolicyContext::Default => {
                    Policy::DefaultContext(rules_try_from_rule_elements(rules)?)
                }
                PolicyContext::Mandatory => {
                    Policy::MandatoryContext(rules_try_from_rule_elements(rules)?)
                }
            })),
            PolicyElement {
                at_console: None,
                context: None,
                group: Some(group),
                rules,
                user: None,
            } => Ok(Some(Policy::Group(
                rules_try_from_rule_elements(rules)?,
                group,
            ))),
            PolicyElement {
                at_console: None,
                context: None,
                group: None,
                rules,
                user: Some(user),
            } => Ok(Some(Policy::User(
                rules_try_from_rule_elements(rules)?,
                user,
            ))),
            _ => Err(Error::msg(format!(
                "policy contains conflicting attributes: {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ReceiveOperation {
    pub error: Option<String>,
    pub interface: Option<String>,
    pub max_fds: Option<u32>,
    pub member: Option<String>,
    pub min_fds: Option<u32>,
    pub path: Option<String>,
    pub sender: Option<String>,
    pub r#type: Option<MessageType>,
}

impl From<RuleAttributes> for ReceiveOperation {
    fn from(value: RuleAttributes) -> Self {
        Self {
            error: value.receive_error,
            interface: value.receive_interface,
            max_fds: value.max_fds,
            member: value.receive_member,
            min_fds: value.min_fds,
            path: value.receive_path,
            sender: value.receive_sender,
            r#type: value.receive_type,
        }
    }
}

type OptionalRule = Option<Rule>;

impl TryFrom<RuleElement> for OptionalRule {
    type Error = Error;

    fn try_from(value: RuleElement) -> std::result::Result<Self, Self::Error> {
        match value {
            RuleElement::Allow(RuleAttributes {
                group: Some(_),
                user: Some(_),
                ..
            })
            | RuleElement::Deny(RuleAttributes {
                group: Some(_),
                user: Some(_),
                ..
            }) => Err(Error::msg(format!(
                "`group` cannot be combined with `user` in the same rule: {value:?}"
            ))),
            RuleElement::Allow(RuleAttributes {
                own: Some(_),
                own_prefix: Some(_),
                ..
            })
            | RuleElement::Deny(RuleAttributes {
                own: Some(_),
                own_prefix: Some(_),
                ..
            }) => Err(Error::msg(format!(
                "`own_prefix` cannot be combined with `own` in the same rule: {value:?}"
            ))),
            RuleElement::Allow(RuleAttributes {
                send_destination: Some(_),
                send_destination_prefix: Some(_),
                ..
            })
            | RuleElement::Deny(RuleAttributes {
                send_destination: Some(_),
                send_destination_prefix: Some(_),
                ..
            }) => Err(Error::msg(format!(
                "`send_destination_prefix` cannot be combined with `send_destination` in the same rule: {value:?}"
            ))),
            RuleElement::Allow(RuleAttributes {
                eavesdrop: Some(true),
                group: None,
                own: None,
                receive_requested_reply: None,
                receive_sender: None,
                send_broadcast: None,
                send_destination: None,
                send_destination_prefix: None,
                send_error: None,
                send_interface: None,
                send_member: None,
                send_path: None,
                send_requested_reply: None,
                send_type: None,
                user: None,
                ..
            }) => {
                // see: https://github.com/dbus2/busd/pull/146#issuecomment-2408429760
                Ok(None)
            }
            RuleElement::Allow(
                RuleAttributes {
                    receive_requested_reply: Some(false),
                    ..
                }
                | RuleAttributes {
                    send_requested_reply: Some(false),
                    ..
                },
            ) => {
                // see: https://github.com/dbus2/busd/pull/146#issuecomment-2408429760
                Ok(None)
            }
            RuleElement::Allow(attrs) => {
                // if attrs.eavesdrop == Some(true) {
                // see: https://github.com/dbus2/busd/pull/146#issuecomment-2408429760
                // }
                match OptionalOperation::try_from(attrs)? {
                    Some(some) => Ok(Some((Access::Allow, some))),
                    None => Ok(None),
                }
            }
            RuleElement::Deny(RuleAttributes {
                eavesdrop: Some(true),
                ..
            }) => {
                // see: https://github.com/dbus2/busd/pull/146#issuecomment-2408429760
                Ok(None)
            }
            RuleElement::Deny(
                RuleAttributes {
                    receive_requested_reply: Some(true),
                    ..
                }
                | RuleAttributes {
                    send_requested_reply: Some(true),
                    ..
                },
            ) => {
                // see: https://github.com/dbus2/busd/pull/146#issuecomment-2408429760
                Ok(None)
            }
            RuleElement::Deny(attrs) => match OptionalOperation::try_from(attrs)? {
                Some(some) => Ok(Some((Access::Deny, some))),
                None => Ok(None),
            },
        }
    }
}

pub type Rule = (Access, Operation);

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum Access {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SendOperation {
    pub broadcast: Option<bool>,
    pub destination: Option<Name>,
    pub error: Option<String>,
    pub interface: Option<String>,
    pub max_fds: Option<u32>,
    pub member: Option<String>,
    pub min_fds: Option<u32>,
    pub path: Option<String>,
    pub r#type: Option<MessageType>,
}

impl From<RuleAttributes> for SendOperation {
    fn from(value: RuleAttributes) -> Self {
        let destination = match value {
            RuleAttributes {
                send_destination: Some(some),
                send_destination_prefix: None,
                ..
            } if some == "*" => Some(Name::Any),
            RuleAttributes {
                send_destination: Some(some),
                send_destination_prefix: None,
                ..
            } => Some(Name::Exact(some)),
            RuleAttributes {
                send_destination: None,
                send_destination_prefix: Some(some),
                ..
            } => Some(Name::Prefix(some)),
            _ => None,
        };
        Self {
            broadcast: value.send_broadcast,
            destination,
            error: value.send_error,
            interface: value.send_interface,
            max_fds: value.max_fds,
            member: value.send_member,
            min_fds: value.min_fds,
            path: value.send_path,
            r#type: value.send_type,
        }
    }
}

const DEFAULT_DATA_DIRS: &[&str] = &["/usr/local/share", "/usr/share"];

const STANDARD_SYSTEM_SERVICEDIRS: &[&str] = &[
    "/usr/local/share/dbus-1/system-services",
    "/usr/share/dbus-1/system-services",
    "/lib/dbus-1/system-services",
];

fn rules_try_from_rule_elements(value: Vec<RuleElement>) -> Result<Vec<Rule>> {
    let mut rules = vec![];
    for rule in value {
        let rule = OptionalRule::try_from(rule)?;
        if let Some(some) = rule {
            rules.push(some);
        }
    }
    Ok(rules)
}

fn xdg_data_dirs() -> Vec<PathBuf> {
    if let Ok(ok) = var("XDG_DATA_DIRS") {
        return ok.split(":").map(PathBuf::from).collect();
    }
    DEFAULT_DATA_DIRS.iter().map(PathBuf::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parse_with_dtd_and_root_element_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig></busconfig>
        "#;
        Config::parse(input).expect("should parse XML input");
    }

    #[test]
    #[should_panic]
    fn config_parse_with_type_error() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <type>not-a-valid-message-bus-type</type>
        </busconfig>
        "#;
        Config::parse(input).expect("should parse XML input");
    }

    #[test]
    fn config_parse_with_allow_anonymous_and_fork_and_keep_umask_and_syslog_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <allow_anonymous />
            <fork />
            <keep_umask/>
            <syslog />
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                allow_anonymous: true,
                fork: true,
                keep_umask: true,
                syslog: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_auth_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <auth>ANONYMOUS</auth>
            <auth>EXTERNAL</auth>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                auth: Some(AuthMechanism::External),
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_limit_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <limit name="max_incoming_bytes">1000000000</limit>
        </busconfig>
        "#;

        Config::parse(input).expect("should parse XML input");
    }

    #[test]
    fn config_parse_with_listen_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <listen>unix:path=/tmp/foo</listen>
            <listen>tcp:host=localhost,port=1234</listen>
            <listen>tcp:host=localhost,port=0,family=ipv4</listen>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                listen: Some(
                    Address::from_str("tcp:host=localhost,port=0,family=ipv4")
                        .expect("should parse address")
                ),
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_overlapped_lists_ok() {
        // confirm this works with/without quick-xml's [`overlapped-lists`] feature
        // [`overlapped-lists`]: https://docs.rs/quick-xml/latest/quick_xml/#overlapped-lists
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <auth>ANONYMOUS</auth>
            <listen>unix:path=/tmp/foo</listen>
            <policy context="default">
                <allow own="*"/>
                <deny own="*"/>
                <allow own="*"/>
            </policy>
            <auth>EXTERNAL</auth>
            <listen>tcp:host=localhost,port=1234</listen>
            <policy context="default">
                <deny own="*"/>
                <allow own="*"/>
                <deny own="*"/>
            </policy>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                auth: Some(AuthMechanism::External),
                listen: Some(
                    Address::from_str("tcp:host=localhost,port=1234")
                        .expect("should parse address")
                ),
                policies: vec![
                    Policy::DefaultContext(vec![
                        (
                            Access::Allow,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Any)
                            })
                        ),
                        (
                            Access::Deny,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Any)
                            })
                        ),
                        (
                            Access::Allow,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Any)
                            })
                        ),
                    ]),
                    Policy::DefaultContext(vec![
                        (
                            Access::Deny,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Any)
                            })
                        ),
                        (
                            Access::Allow,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Any)
                            })
                        ),
                        (
                            Access::Deny,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Any)
                            })
                        ),
                    ]),
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_pidfile_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <pidfile>/var/run/busd.pid</pidfile>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                pidfile: Some(PathBuf::from("/var/run/busd.pid")),
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_policies_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy context="default">
                <allow own="org.freedesktop.DBus"/>
                <allow own_prefix="org.freedesktop"/>
                <allow group="wheel" />
                <allow user="root" />
            </policy>
            <policy user="root">
                <allow
                    send_broadcast="true"
                    send_destination="org.freedesktop.DBus"
                    send_error="something bad"
                    send_interface="org.freedesktop.systemd1.Activator"
                    send_member="DoSomething"
                    send_path="/org/freedesktop"
                    send_type="signal"
                    max_fds="128"
                    min_fds="12"
                    />
                <allow
                    receive_error="something bad"
                    receive_interface="org.freedesktop.systemd1.Activator"
                    receive_member="DoSomething"
                    receive_path="/org/freedesktop"
                    receive_sender="org.freedesktop.DBus"
                    receive_type="signal"
                    max_fds="128"
                    min_fds="12"
                    />
            </policy>
            <policy group="network">
                <allow send_destination_prefix="org.freedesktop" send_member="DoSomething" />
                <allow receive_sender="org.freedesktop.Avahi" receive_member="DoSomething"/>
            </policy>
            <policy context="mandatory">
                <deny send_destination="net.connman.iwd"/>
            </policy>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                policies: vec![
                    Policy::DefaultContext(vec![
                        (
                            Access::Allow,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Exact(String::from("org.freedesktop.DBus")))
                            })
                        ),
                        (
                            Access::Allow,
                            Operation::Own(NameOwnership {
                                own: Some(Name::Prefix(String::from("org.freedesktop")))
                            })
                        ),
                        (
                            Access::Allow,
                            Operation::Connect(ConnectOperation {
                                group: Some(String::from("wheel")),
                                user: None,
                            })
                        ),
                        (
                            Access::Allow,
                            Operation::Connect(ConnectOperation {
                                group: None,
                                user: Some(String::from("root")),
                            })
                        ),
                    ]),
                    Policy::User(
                        vec![
                            (
                                Access::Allow,
                                Operation::Send(SendOperation {
                                    broadcast: Some(true),
                                    destination: Some(Name::Exact(String::from(
                                        "org.freedesktop.DBus"
                                    ))),
                                    error: Some(String::from("something bad")),
                                    interface: Some(String::from(
                                        "org.freedesktop.systemd1.Activator"
                                    )),
                                    max_fds: Some(128),
                                    member: Some(String::from("DoSomething")),
                                    min_fds: Some(12),
                                    path: Some(String::from("/org/freedesktop")),
                                    r#type: Some(MessageType::Signal),
                                })
                            ),
                            (
                                Access::Allow,
                                Operation::Receive(ReceiveOperation {
                                    error: Some(String::from("something bad")),
                                    interface: Some(String::from(
                                        "org.freedesktop.systemd1.Activator"
                                    )),
                                    max_fds: Some(128),
                                    member: Some(String::from("DoSomething")),
                                    min_fds: Some(12),
                                    path: Some(String::from("/org/freedesktop")),
                                    sender: Some(String::from("org.freedesktop.DBus")),
                                    r#type: Some(MessageType::Signal),
                                })
                            )
                        ],
                        String::from("root")
                    ),
                    Policy::Group(
                        vec![
                            (
                                Access::Allow,
                                Operation::Send(SendOperation {
                                    broadcast: None,
                                    destination: Some(Name::Prefix(String::from(
                                        "org.freedesktop"
                                    ))),
                                    error: None,
                                    interface: None,
                                    max_fds: None,
                                    member: Some(String::from("DoSomething")),
                                    min_fds: None,
                                    path: None,
                                    r#type: None
                                })
                            ),
                            // `<allow send_member=...` should be dropped
                            (
                                Access::Allow,
                                Operation::Receive(ReceiveOperation {
                                    sender: Some(String::from("org.freedesktop.Avahi")),
                                    error: None,
                                    interface: None,
                                    max_fds: None,
                                    member: Some(String::from("DoSomething")),
                                    min_fds: None,
                                    path: None,
                                    r#type: None
                                })
                            ),
                        ],
                        String::from("network")
                    ),
                    Policy::MandatoryContext(vec![(
                        Access::Deny,
                        Operation::Send(SendOperation {
                            broadcast: None,
                            destination: Some(Name::Exact(String::from("net.connman.iwd"))),
                            error: None,
                            interface: None,
                            max_fds: None,
                            member: None,
                            min_fds: None,
                            path: None,
                            r#type: None
                        })
                    ),]),
                ],
                ..Default::default()
            }
        );
    }

    #[should_panic]
    #[test]
    fn config_parse_with_policies_with_group_and_user_error() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy user="root">
                <allow group="wheel" user="root" />
            </policy>
        </busconfig>
        "#;

        Config::parse(input).expect("should parse XML input");
    }

    #[test]
    fn config_parse_with_policies_with_ignored_rules_and_rule_attributes_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy context="default">
                <allow send_destination="*" eavesdrop="true"/>
                <allow eavesdrop="true"/>
                <deny eavesdrop="true"/>
                <deny send_requested_reply="true" send_type="method_return"/>
                <allow send_requested_reply="false" send_type="method_return"/>
                <deny receive_requested_reply="true" receive_type="error"/>
                <allow receive_requested_reply="false" receive_type="error"/>
            </policy>
            <policy at_console="true">
                <allow send_destination="org.freedesktop.DBus" send_interface="org.freedesktop.systemd1.Activator"/>
            </policy>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                policies: vec![
                    Policy::DefaultContext(vec![
                        (
                            Access::Allow,
                            // `eavesdrop="true"` is dropped, keep other attributes
                            Operation::Send(SendOperation {
                                broadcast: None,
                                destination: Some(Name::Any),
                                error: None,
                                interface: None,
                                max_fds: None,
                                member: None,
                                min_fds: None,
                                path: None,
                                r#type: None
                            })
                        ),
                        // `<allow eavesdrop="true"/>` has nothing left after dropping eavesdrop
                        // `<deny eavesdrop="true" ...` is completely ignored
                        // `<deny send_requested_reply="true" ...` is completely ignored
                        // `<allow send_requested_reply="false" ...` is completely ignored
                        // `<deny receive_requested_reply="true" ...` is completely ignored
                        // `<allow receive_requested_reply="false" ...` is completely ignored
                    ]),
                    // `<policy at_console="true">` is completely ignored
                ],
                ..Default::default()
            }
        );
    }

    #[should_panic]
    #[test]
    fn config_parse_with_policies_with_own_and_own_prefix_error() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy user="root">
                <allow own="org.freedesktop.DBus" own_prefix="org.freedesktop" />
            </policy>
        </busconfig>
        "#;

        Config::parse(input).expect("should parse XML input");
    }

    #[should_panic]
    #[test]
    fn config_parse_with_policies_with_send_destination_and_send_destination_prefix_error() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy user="root">
                <allow send_destination="org.freedesktop.DBus" send_destination_prefix="org.freedesktop" />
            </policy>
        </busconfig>
        "#;

        Config::parse(input).expect("should parse XML input");
    }

    #[should_panic]
    #[test]
    fn config_parse_with_policies_with_send_and_receive_attributes_error() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy user="root">
                <allow send_destination="org.freedesktop.DBus" receive_sender="org.freedesktop.Avahi" />
            </policy>
        </busconfig>
        "#;

        Config::parse(input).expect("should parse XML input");
    }

    #[should_panic]
    #[test]
    fn config_parse_with_policies_without_attributes_error() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <policy user="root">
                <allow />
            </policy>
        </busconfig>
        "#;

        Config::parse(input).expect("should parse XML input");
    }

    #[test]
    fn config_parse_with_servicedir_and_standard_session_servicedirs_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <servicedir>/example</servicedir>
            <standard_session_servicedirs />
            <servicedir>/anotherexample</servicedir>
            <standard_session_servicedirs />
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        // TODO: improve test: contents are dynamic depending upon environment variables
        assert_eq!(config.servicedirs.first(), Some(&PathBuf::from("/example")));
        assert_eq!(
            config.servicedirs.last(),
            Some(&PathBuf::from("/usr/share/dbus-1/services"))
        );
    }

    #[test]
    fn config_parse_with_servicedir_and_standard_system_servicedirs_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <servicedir>/example</servicedir>
            <standard_system_servicedirs />
            <servicedir>/anotherexample</servicedir>
            <standard_system_servicedirs />
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                servicedirs: vec![
                    PathBuf::from("/example"),
                    PathBuf::from("/usr/local/share/dbus-1/system-services"),
                    PathBuf::from("/usr/share/dbus-1/system-services"),
                    PathBuf::from("/lib/dbus-1/system-services"),
                    PathBuf::from("/anotherexample"),
                    PathBuf::from("/usr/local/share/dbus-1/system-services"),
                    PathBuf::from("/usr/share/dbus-1/system-services"),
                    PathBuf::from("/lib/dbus-1/system-services"),
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_servicehelper_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <servicehelper>/example</servicehelper>
            <servicehelper>/anotherexample</servicehelper>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                servicehelper: Some(PathBuf::from("/anotherexample")),
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_type_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <type>session</type>
            <type>system</type>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                r#type: Some(BusType::System),
                ..Default::default()
            }
        );
    }

    #[test]
    fn config_parse_with_user_ok() {
        let input = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
            <user>1000</user>
            <user>alice</user>
        </busconfig>
        "#;

        let config = Config::parse(input).expect("should parse XML input");

        assert_eq!(
            config,
            Config {
                user: Some(String::from("alice")),
                ..Default::default()
            }
        );
    }
}
