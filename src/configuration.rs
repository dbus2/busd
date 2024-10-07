//! implementation of "Configuration File" described at:
//! https://dbus.freedesktop.org/doc/dbus-daemon.1.html

use std::{path::PathBuf, str::FromStr};

use raw::{RawConfiguration, RawPolicy, RawPolicyContext, RawRule, RawRuleAttributes};
use serde::Deserialize;

mod raw;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ApparmorMode {
    Disabled,
    #[default]
    Enabled,
    Required,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Associate {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "@own")]
    own: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Configuration {
    allow_anonymous: Option<bool>,
    apparmor: ApparmorMode,
    auth: Vec<String>,
    fork: Option<bool>,
    // TODO: consider processing `include` more to remove XML-specific structure
    include: Vec<IncludeElement>,
    includedir: Vec<PathBuf>,
    keep_umask: Option<bool>,
    // TODO: consider processing `include` more to remove XML-specific structure
    limit: Vec<LimitElement>,
    listen: Vec<String>,
    pidfile: Option<PathBuf>,
    policy: Vec<Policy>,
    selinux: Vec<Associate>,
    servicedir: Vec<PathBuf>,
    servicehelper: Option<PathBuf>,
    standard_session_servicedirs: Option<bool>,
    standard_system_servicedirs: Option<bool>,
    syslog: Option<bool>,
    r#type: Option<Type>,
    user: Option<Principal>,
}
impl FromStr for Configuration {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RawConfiguration::from_str(s)
            .map_err(Error::DeserializeXml)
            .and_then(Self::try_from)
    }
}
impl TryFrom<RawConfiguration> for Configuration {
    type Error = Error;

    fn try_from(value: RawConfiguration) -> Result<Self, Self::Error> {
        let mut policy = Vec::with_capacity(value.policy.len());
        for rp in value.policy {
            match Policy::try_from(rp) {
                Ok(p) => policy.push(p),
                Err(err) => {
                    return Err(err);
                }
            }
        }

        let mut bc = Self {
            allow_anonymous: value.allow_anonymous.map(|_| true),
            apparmor: match value.apparmor {
                Some(a) => a.mode,
                None => None,
            }
            .unwrap_or_default(),
            auth: value.auth,
            fork: value.fork.map(|_| true),
            include: value.include,
            includedir: value.includedir.into_iter().map(|pb| pb.text).collect(),
            keep_umask: value.keep_umask.map(|_| true),
            limit: value.limit,
            listen: value.listen,
            pidfile: value.pidfile,
            policy,
            // TODO: SELinux could probably more-conveniently be represented as a HashMap
            // TODO: last one wins for SELinux associates with the same name
            selinux: match value.selinux {
                Some(s) => s.associate,
                None => vec![],
            },
            servicedir: value.servicedir.into_iter().map(|pb| pb.text).collect(),
            servicehelper: value.servicehelper,
            standard_session_servicedirs: value.standard_session_servicedirs.map(|_| true),
            standard_system_servicedirs: value.standard_system_servicedirs.map(|_| true),
            syslog: value.syslog.map(|_| true),
            ..Default::default()
        };

        // > The last element "wins"
        if let Some(te) = value.r#type.into_iter().last() {
            bc.r#type = Some(te.text);
        }
        if let Some(ue) = value.user.into_iter().last() {
            bc.user = Some(ue.text);
        }

        Ok(bc)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectRule {
    group: Option<RuleMatch>,
    user: Option<RuleMatch>,
}

#[derive(Clone, Debug)]
pub enum Error {
    DeserializeXml(quick_xml::DeError),
    PolicyHasMultipleAttributes,
    RuleHasInvalidCombinationOfAttributes,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IgnoreMissing {
    No,
    Yes,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub struct IncludeElement {
    #[serde(rename = "@ignore_missing")]
    ignore_missing: Option<IgnoreMissing>,
    #[serde(rename = "$text")]
    text: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub struct LimitElement {
    #[serde(rename = "@name")]
    name: LimitName,
    #[serde(rename = "$text")]
    text: i32, // semantically should be u32, but i32 for compatibility
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LimitName {
    AuthTimeout,
    MaxCompletedConnections,
    MaxConnectionsPerUser,
    MaxIncomingBytes,
    MaxIncomingUnixFds,
    MaxIncompleteConnections,
    MaxMatchRulesPerConnection,
    MaxMessageSize,
    MaxMessageUnixFds,
    MaxNamesPerConnection,
    MaxOutgoingBytes,
    MaxOutgoingUnixFds,
    MaxPendingServiceStarts,
    MaxRepliesPerConnection,
    PendingFdTimeout,
    ServiceStartTimeout,
    ReplyTimeout,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnRule {
    own: Option<RuleMatch>,
    own_prefix: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Policy {
    Console { rules: Vec<Rule> },
    DefaultContext { rules: Vec<Rule> },
    Group { group: Principal, rules: Vec<Rule> },
    MandatoryContext { rules: Vec<Rule> },
    NoConsole { rules: Vec<Rule> },
    User { user: Principal, rules: Vec<Rule> },
}
impl TryFrom<RawPolicy> for Policy {
    type Error = Error;
    fn try_from(value: RawPolicy) -> Result<Self, Self::Error> {
        let mut rules: Vec<Rule> = Vec::with_capacity(value.rules.len());
        for rule in value.rules {
            match Rule::try_from(rule) {
                Ok(ok) => rules.push(ok),
                Err(err) => return Err(err),
            }
        }

        match value {
            RawPolicy {
                at_console: Some(b),
                context: None,
                group: None,
                user: None,
                ..
            } => Ok(match b {
                true => Self::Console { rules },
                false => Self::NoConsole { rules },
            }),
            RawPolicy {
                at_console: None,
                context: Some(pc),
                group: None,
                user: None,
                ..
            } => Ok(match pc {
                RawPolicyContext::Default => Self::DefaultContext { rules },
                RawPolicyContext::Mandatory => Self::MandatoryContext { rules },
            }),
            RawPolicy {
                at_console: None,
                context: None,
                group: Some(p),
                user: None,
                ..
            } => Ok(Self::Group { group: p, rules }),
            RawPolicy {
                at_console: None,
                context: None,
                group: None,
                user: Some(p),
                ..
            } => Ok(Self::User { user: p, rules }),
            _ => Err(Error::PolicyHasMultipleAttributes),
        }
    }
}
// TODO: impl PartialOrd/Ord for Policy, for order in which policies are applied to a connection

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase", untagged)]
pub enum Principal {
    Id(u32),
    Name(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReceiveRule {
    eavesdrop: Option<bool>,
    receive_error: Option<RuleMatch>,
    receive_interface: Option<RuleMatch>,
    receive_member: Option<RuleMatch>,
    receive_path: Option<RuleMatch>,
    receive_requested_reply: Option<bool>,
    receive_sender: Option<RuleMatch>,
    receive_type: Option<RuleMatchType>,
}

pub type Rule = (RuleEffect, RulePhase);
impl TryFrom<RawRule> for Rule {
    type Error = Error;

    fn try_from(value: RawRule) -> Result<Self, Self::Error> {
        let (effect, attributes) = match value {
            RawRule::Allow(attributes) => (RuleEffect::Allow, attributes),
            RawRule::Deny(attributes) => (RuleEffect::Deny, attributes),
        };
        match attributes {
            RawRuleAttributes {
                eavesdrop,
                group: None,
                own: None,
                own_prefix: None,
                receive_error,
                receive_interface,
                receive_member,
                receive_path,
                receive_requested_reply,
                receive_sender,
                receive_type,
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
            } => Ok((
                effect,
                RulePhase::Receive(ReceiveRule {
                    eavesdrop,
                    receive_error,
                    receive_interface,
                    receive_member,
                    receive_path,
                    receive_requested_reply,
                    receive_sender,
                    receive_type,
                }),
            )),
            RawRuleAttributes {
                eavesdrop,
                group: None,
                own: None,
                own_prefix: None,
                receive_error: None,
                receive_interface: None,
                receive_member: None,
                receive_path: None,
                receive_requested_reply: None,
                receive_sender: None,
                receive_type: None,
                send_broadcast,
                send_destination,
                send_destination_prefix,
                send_error,
                send_interface,
                send_member,
                send_path,
                send_requested_reply,
                send_type,
                user: None,
            } => Ok((
                effect,
                RulePhase::Send(SendRule {
                    eavesdrop,
                    send_broadcast,
                    send_destination,
                    send_destination_prefix,
                    send_error,
                    send_interface,
                    send_member,
                    send_path,
                    send_requested_reply,
                    send_type,
                }),
            )),
            RawRuleAttributes {
                eavesdrop: None,
                group: None,
                own,
                own_prefix,
                receive_error: None,
                receive_interface: None,
                receive_member: None,
                receive_path: None,
                receive_requested_reply: None,
                receive_sender: None,
                receive_type: None,
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
            } => Ok((effect, RulePhase::Own(OwnRule { own, own_prefix }))),
            RawRuleAttributes {
                eavesdrop: None,
                group,
                own: None,
                own_prefix: None,
                receive_error: None,
                receive_interface: None,
                receive_member: None,
                receive_path: None,
                receive_requested_reply: None,
                receive_sender: None,
                receive_type: None,
                send_broadcast: None,
                send_destination: None,
                send_destination_prefix: None,
                send_error: None,
                send_interface: None,
                send_member: None,
                send_path: None,
                send_requested_reply: None,
                send_type: None,
                user,
            } => Ok((effect, RulePhase::Connect(ConnectRule { group, user }))),
            _ => Err(Error::RuleHasInvalidCombinationOfAttributes),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuleEffect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RulePhase {
    Connect(ConnectRule),
    Own(OwnRule),
    Receive(ReceiveRule),
    Send(SendRule),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", untagged)]
pub enum RuleMatch {
    #[serde(rename = "*")]
    Any,
    One(String),
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchType {
    #[serde(rename = "*")]
    Any,
    Error,
    MethodCall,
    MethodReturn,
    Signal,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SendRule {
    eavesdrop: Option<bool>,
    send_broadcast: Option<bool>,
    send_destination: Option<RuleMatch>,
    send_destination_prefix: Option<String>,
    send_error: Option<RuleMatch>,
    send_interface: Option<RuleMatch>,
    send_member: Option<RuleMatch>,
    send_path: Option<RuleMatch>,
    send_requested_reply: Option<bool>,
    send_type: Option<RuleMatchType>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    Session,
    System,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busconfig_fromstr_last_type_wins_ok() {
        let input = r#"
                <!DOCTYPE busconfig PUBLIC
     "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
        <type>system</type>
        <type>session</type>
    </busconfig>
            "#;

        let got = Configuration::from_str(input).expect("should parse input XML");

        assert_eq!(got.r#type, Some(Type::Session));
    }

    #[test]
    fn busconfig_fromstr_last_user_wins_ok() {
        let input = r#"
                <!DOCTYPE busconfig PUBLIC
     "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
        <user>1234</user>
        <user>nobody</user>
    </busconfig>
            "#;

        let got = Configuration::from_str(input).expect("should parse input XML");

        assert_eq!(got.user, Some(Principal::Name(String::from("nobody"))));
    }

    #[test]
    fn busconfig_fromstr_allow_deny_allow_ok() {
        // from https://github.com/OpenPrinting/system-config-printer/blob/caa1ba33da20fd2a82cee0bcc97589fede512cc8/dbus/com.redhat.PrinterDriversInstaller.conf
        // selected because it has a <deny /> in the middle of a list of <allow />s
        let input = r#"
            <!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
	<policy user="root">
		<allow send_destination="com.redhat.PrinterDriversInstaller"
		       send_interface="com.redhat.PrinterDriversInstaller"/>
	</policy>

	<policy context="default">
		<allow own="com.redhat.PrinterDriversInstaller"/>

		<deny send_destination="com.redhat.PrinterDriversInstaller"
		      send_interface="com.redhat.PrinterDriversInstaller"/>
		<allow send_destination="com.redhat.PrinterDriversInstaller"
		       send_interface="org.freedesktop.DBus.Introspectable" />
		<allow send_destination="com.redhat.PrinterDriversInstaller"
		       send_interface="org.freedesktop.DBus.Properties" />
	</policy>
</busconfig>
        "#;

        let got = Configuration::from_str(input).expect("should parse input XML");

        assert_eq!(
            got,
            Configuration {
                policy: vec![
                    Policy::User {
                        rules: vec![(
                            RuleEffect::Allow,
                            RulePhase::Send(SendRule {
                                send_destination: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                send_interface: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                ..Default::default()
                            })
                        )],
                        user: Principal::Name(String::from("root")),
                    },
                    Policy::DefaultContext {
                        rules: vec![
                            (
                                RuleEffect::Allow,
                                RulePhase::Own(OwnRule {
                                    own: Some(RuleMatch::One(String::from(
                                        "com.redhat.PrinterDriversInstaller"
                                    ))),
                                    ..Default::default()
                                })
                            ),
                            (
                                RuleEffect::Deny,
                                RulePhase::Send(SendRule {
                                    send_destination: Some(RuleMatch::One(String::from(
                                        "com.redhat.PrinterDriversInstaller"
                                    ))),
                                    send_interface: Some(RuleMatch::One(String::from(
                                        "com.redhat.PrinterDriversInstaller"
                                    ))),
                                    ..Default::default()
                                })
                            ),
                            (
                                RuleEffect::Allow,
                                RulePhase::Send(SendRule {
                                    send_destination: Some(RuleMatch::One(String::from(
                                        "com.redhat.PrinterDriversInstaller"
                                    ))),
                                    send_interface: Some(RuleMatch::One(String::from(
                                        "org.freedesktop.DBus.Introspectable"
                                    ))),
                                    ..Default::default()
                                })
                            ),
                            (
                                RuleEffect::Allow,
                                RulePhase::Send(SendRule {
                                    send_destination: Some(RuleMatch::One(String::from(
                                        "com.redhat.PrinterDriversInstaller"
                                    ))),
                                    send_interface: Some(RuleMatch::One(String::from(
                                        "org.freedesktop.DBus.Properties"
                                    ))),
                                    ..Default::default()
                                })
                            ),
                        ]
                    }
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn busconfig_fromstr_limit_ok() {
        let input = r#"
            <!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
     <limit name="max_incoming_bytes">133169152</limit>
     <limit name="max_incoming_unix_fds">64</limit>
</busconfig>
        "#;

        let got = Configuration::from_str(input).expect("should parse input XML");

        assert_eq!(
            got,
            Configuration {
                limit: vec![
                    LimitElement {
                        name: LimitName::MaxIncomingBytes,
                        text: 133169152
                    },
                    LimitElement {
                        name: LimitName::MaxIncomingUnixFds,
                        text: 64
                    },
                ],
                ..Default::default()
            }
        );
    }

    #[test]
    fn busconfig_fromstr_apparmor_and_selinux_ok() {
        let input = r#"
            <!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
    <apparmor mode="required" />
    <selinux>
        <associate own="org.freedesktop.Foobar" context="foo_t" />
    </selinux>
</busconfig>
        "#;

        let got = Configuration::from_str(input).expect("should parse input XML");

        assert_eq!(
            got,
            Configuration {
                apparmor: ApparmorMode::Required,
                selinux: vec![Associate {
                    context: String::from("foo_t"),
                    own: String::from("org.freedesktop.Foobar")
                },],
                ..Default::default()
            }
        );
    }

    #[test]
    fn busconfig_fromstr_receiverule_ok() {
        // from https://github.com/OpenPrinting/system-config-printer/blob/caa1ba33da20fd2a82cee0bcc97589fede512cc8/dbus/com.redhat.PrinterDriversInstaller.conf
        // selected because it has a <deny /> in the middle of a list of <allow />s
        let input = r#"
            <!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
	<policy context="default">
		<allow eavesdrop="false" />
		<allow eavesdrop="true" />
		<deny eavesdrop="false" receive_requested_reply="true" />
		<deny eavesdrop="true" receive_requested_reply="true" />
	</policy>
</busconfig>
        "#;

        let got = Configuration::from_str(input).expect("should parse input XML");

        assert_eq!(
            got,
            Configuration {
                policy: vec![Policy::DefaultContext {
                    rules: vec![
                        (
                            RuleEffect::Allow,
                            RulePhase::Receive(ReceiveRule {
                                eavesdrop: Some(false),
                                ..Default::default()
                            })
                        ),
                        (
                            RuleEffect::Allow,
                            RulePhase::Receive(ReceiveRule {
                                eavesdrop: Some(true),
                                ..Default::default()
                            })
                        ),
                        (
                            RuleEffect::Deny,
                            RulePhase::Receive(ReceiveRule {
                                eavesdrop: Some(false),
                                receive_requested_reply: Some(true),
                                ..Default::default()
                            })
                        ),
                        (
                            RuleEffect::Deny,
                            RulePhase::Receive(ReceiveRule {
                                eavesdrop: Some(true),
                                receive_requested_reply: Some(true),
                                ..Default::default()
                            })
                        ),
                    ]
                }],
                ..Default::default()
            }
        );
    }
}
