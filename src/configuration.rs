//! implementation of "Configuration File" described at:
//! https://dbus.freedesktop.org/doc/dbus-daemon.1.html

use std::{path::PathBuf, str::FromStr};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct Apparmor {
    #[serde(rename = "@mode")]
    mode: Option<ApparmorMode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ApparmorMode {
    Disabled,
    Enabled,
    Required,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Associate {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "@own")]
    own: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Configuration {
    allow_anonymous: Option<bool>,
    apparmor: Option<ApparmorMode>,
    auth: Vec<String>,
    fork: Option<bool>,
    include: Vec<IncludeElement>,
    includedir: Vec<PathBufElement>,
    keep_umask: Option<bool>,
    limit: Vec<LimitElement>,
    listen: Vec<String>,
    pidfile: Option<PathBuf>,
    policy: Vec<Policy>,
    selinux: Vec<Associate>,
    servicedir: Vec<PathBufElement>,
    servicehelper: Option<PathBuf>,
    standard_session_servicedirs: Option<bool>,
    standard_system_servicedirs: Option<bool>,
    syslog: Option<bool>,
    r#type: Option<Type>,
    user: Option<Principal>,
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
            },
            auth: value.auth,
            fork: value.fork.map(|_| true),
            include: value.include,
            includedir: value.includedir,
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
            servicedir: value.servicedir,
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

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    PolicyHasMultipleAttributes,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum IgnoreMissing {
    No,
    Yes,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Include {
    Session,
    System,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct IncludeElement {
    #[serde(rename = "@ignore_missing")]
    ignore_missing: Option<IgnoreMissing>,
    #[serde(rename = "$text")]
    text: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct LimitElement {
    #[serde(rename = "@name")]
    name: LimitName,
    #[serde(rename = "$text")]
    text: i32, // semantically should be u32, but i32 for compatibility
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LimitName {
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

// reuse this between Vec<PathBuf> fields,
// except those with field-specific attributes
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct PathBufElement {
    #[serde(rename = "$text")]
    text: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
enum Policy {
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
        // TODO: more validations and conversions as documented against dbus-daemon
        match value {
            RawPolicy {
                at_console: Some(b),
                context: None,
                group: None,
                rules,
                user: None,
            } => Ok(match b {
                true => Self::Console { rules },
                false => Self::NoConsole { rules },
            }),
            RawPolicy {
                at_console: None,
                context: Some(pc),
                group: None,
                rules,
                user: None,
            } => Ok(match pc {
                PolicyContext::Default => Self::DefaultContext { rules },
                PolicyContext::Mandatory => Self::MandatoryContext { rules },
            }),
            RawPolicy {
                at_console: None,
                context: None,
                group: Some(p),
                rules,
                user: None,
            } => Ok(Self::Group { group: p, rules }),
            RawPolicy {
                at_console: None,
                context: None,
                group: None,
                rules,
                user: Some(p),
            } => Ok(Self::User { user: p, rules }),
            _ => Err(Error::PolicyHasMultipleAttributes),
        }
    }
}
// TODO: impl PartialOrd/Ord for Policy, for order in which policies are applied to a connection

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PolicyContext {
    Default,
    Mandatory,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
struct PolicyList {
    policy: Vec<RawPolicy>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase", untagged)]
enum Principal {
    Id(u32),
    Name(String),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RawConfiguration {
    allow_anonymous: Option<()>,
    apparmor: Option<Apparmor>,
    auth: Vec<String>,
    fork: Option<()>,
    include: Vec<IncludeElement>,
    includedir: Vec<PathBufElement>,
    keep_umask: Option<()>,
    limit: Vec<LimitElement>,
    listen: Vec<String>,
    pidfile: Option<PathBuf>,
    policy: Vec<RawPolicy>,
    selinux: Option<Selinux>,
    servicedir: Vec<PathBufElement>,
    servicehelper: Option<PathBuf>,
    standard_session_servicedirs: Option<()>,
    standard_system_servicedirs: Option<()>,
    syslog: Option<()>,
    r#type: Vec<TypeElement>,
    user: Vec<UserElement>,
}
impl FromStr for RawConfiguration {
    type Err = quick_xml::DeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // TODO: validate expected DOCTYPE
        // TODO: validate expected root element (busconfig)
        quick_xml::de::from_str(s)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct RawPolicy {
    #[serde(rename = "@at_console")]
    at_console: Option<bool>,
    #[serde(rename = "@context")]
    context: Option<PolicyContext>,
    #[serde(rename = "@group")]
    group: Option<Principal>,
    #[serde(default, rename = "$value")]
    rules: Vec<Rule>,
    #[serde(rename = "@user")]
    user: Option<Principal>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Rule {
    Allow(RuleAttributes),
    Deny(RuleAttributes),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "lowercase")]
struct RuleAttributes {
    #[serde(rename = "@send_interface")]
    send_interface: Option<RuleMatch>,
    #[serde(rename = "@send_member")]
    send_member: Option<RuleMatch>,
    #[serde(rename = "@send_error")]
    send_error: Option<RuleMatch>,
    #[serde(rename = "@send_broadcast")]
    send_broadcast: Option<bool>,
    #[serde(rename = "@send_destination")]
    send_destination: Option<RuleMatch>,
    #[serde(rename = "@send_destination_prefix")]
    send_destination_prefix: Option<String>,
    #[serde(rename = "@send_type")]
    send_type: Option<RuleMatchType>,
    #[serde(rename = "@send_path")]
    send_path: Option<RuleMatch>,
    #[serde(rename = "@receive_interface")]
    receive_interface: Option<RuleMatch>,
    #[serde(rename = "@receive_member")]
    receive_member: Option<RuleMatch>,
    #[serde(rename = "@receive_error")]
    receive_error: Option<RuleMatch>,
    #[serde(rename = "@receive_sender")]
    receive_sender: Option<RuleMatch>,
    #[serde(rename = "@receive_type")]
    receive_type: Option<RuleMatchType>,
    #[serde(rename = "@receive_path")]
    receive_path: Option<RuleMatch>,
    #[serde(rename = "@send_requested_reply")]
    send_requested_reply: Option<bool>,
    #[serde(rename = "@receive_requested_reply")]
    receive_requested_reply: Option<bool>,
    #[serde(rename = "@eavesdrop")]
    eavesdrop: Option<bool>,
    #[serde(rename = "@own")]
    own: Option<RuleMatch>,
    #[serde(rename = "@own_prefix")]
    own_prefix: Option<String>,
    #[serde(rename = "@user")]
    user: Option<RuleMatch>,
    #[serde(rename = "@group")]
    group: Option<RuleMatch>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", untagged)]
enum RuleMatch {
    #[serde(rename = "*")]
    Any,
    One(String),
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RuleMatchType {
    #[serde(rename = "*")]
    Any,
    Error,
    MethodCall,
    MethodReturn,
    Signal,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
struct Selinux {
    associate: Vec<Associate>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Type {
    Session,
    System,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct TypeElement {
    #[serde(rename = "$text")]
    text: Type,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
struct UserElement {
    #[serde(rename = "$text")]
    text: Principal,
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

        let got = RawConfiguration::from_str(input).expect("should parse input XML");
        let got = Configuration::try_from(got).expect("should validate and convert");

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

        let got = RawConfiguration::from_str(input).expect("should parse input XML");
        let got = Configuration::try_from(got).expect("should validate and convert");

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

        let got = RawConfiguration::from_str(input).expect("should parse input XML");
        let got = Configuration::try_from(got).expect("should validate and convert");

        assert_eq!(
            got,
            Configuration {
                policy: vec![
                    Policy::User {
                        rules: vec![Rule::Allow(RuleAttributes {
                            send_destination: Some(RuleMatch::One(String::from(
                                "com.redhat.PrinterDriversInstaller"
                            ))),
                            send_interface: Some(RuleMatch::One(String::from(
                                "com.redhat.PrinterDriversInstaller"
                            ))),
                            ..Default::default()
                        })],
                        user: Principal::Name(String::from("root")),
                    },
                    Policy::DefaultContext {
                        rules: vec![
                            Rule::Allow(RuleAttributes {
                                own: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                ..Default::default()
                            }),
                            Rule::Deny(RuleAttributes {
                                send_destination: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                send_interface: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                ..Default::default()
                            }),
                            Rule::Allow(RuleAttributes {
                                send_destination: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                send_interface: Some(RuleMatch::One(String::from(
                                    "org.freedesktop.DBus.Introspectable"
                                ))),
                                ..Default::default()
                            }),
                            Rule::Allow(RuleAttributes {
                                send_destination: Some(RuleMatch::One(String::from(
                                    "com.redhat.PrinterDriversInstaller"
                                ))),
                                send_interface: Some(RuleMatch::One(String::from(
                                    "org.freedesktop.DBus.Properties"
                                ))),
                                ..Default::default()
                            }),
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

        let got = RawConfiguration::from_str(input).expect("should parse input XML");
        let got = Configuration::try_from(got).expect("should validate and convert");

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
    <apparmor mode="enabled" />
    <selinux>
        <associate own="org.freedesktop.Foobar" context="foo_t" />
    </selinux>
</busconfig>
        "#;

        let got = RawConfiguration::from_str(input).expect("should parse input XML");
        let got = Configuration::try_from(got).expect("should validate and convert");

        assert_eq!(
            got,
            Configuration {
                apparmor: Some(ApparmorMode::Enabled),
                selinux: vec![Associate {
                    context: String::from("foo_t"),
                    own: String::from("org.freedesktop.Foobar")
                },],
                ..Default::default()
            }
        );
    }
}
