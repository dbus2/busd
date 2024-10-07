//! internal implementation details for handling configuration XML

use std::{path::PathBuf, str::FromStr};

use serde::Deserialize;

use super::{
    ApparmorMode, Associate, IncludeElement, LimitElement, PathBufElement, Principal, RuleMatch,
    RuleMatchType, Type,
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) struct RawApparmor {
    #[serde(rename = "@mode")]
    pub mode: Option<ApparmorMode>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct RawConfiguration {
    pub allow_anonymous: Option<()>,
    pub apparmor: Option<RawApparmor>,
    pub auth: Vec<String>,
    pub fork: Option<()>,
    pub include: Vec<IncludeElement>,
    pub includedir: Vec<PathBufElement>,
    pub keep_umask: Option<()>,
    pub limit: Vec<LimitElement>,
    pub listen: Vec<String>,
    pub pidfile: Option<PathBuf>,
    pub policy: Vec<RawPolicy>,
    pub selinux: Option<RawSelinux>,
    pub servicedir: Vec<PathBufElement>,
    pub servicehelper: Option<PathBuf>,
    pub standard_session_servicedirs: Option<()>,
    pub standard_system_servicedirs: Option<()>,
    pub syslog: Option<()>,
    pub r#type: Vec<RawTypeElement>,
    pub user: Vec<RawUserElement>,
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
pub(super) struct RawPolicy {
    #[serde(rename = "@at_console")]
    pub at_console: Option<bool>,
    #[serde(rename = "@context")]
    pub context: Option<RawPolicyContext>,
    #[serde(rename = "@group")]
    pub group: Option<Principal>,
    #[serde(default, rename = "$value")]
    pub rules: Vec<RawRule>,
    #[serde(rename = "@user")]
    pub user: Option<Principal>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum RawPolicyContext {
    Default,
    Mandatory,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum RawRule {
    Allow(RawRuleAttributes),
    Deny(RawRuleAttributes),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "lowercase")]
pub(super) struct RawRuleAttributes {
    #[serde(rename = "@send_interface")]
    pub send_interface: Option<RuleMatch>,
    #[serde(rename = "@send_member")]
    pub send_member: Option<RuleMatch>,
    #[serde(rename = "@send_error")]
    pub send_error: Option<RuleMatch>,
    #[serde(rename = "@send_broadcast")]
    pub send_broadcast: Option<bool>,
    #[serde(rename = "@send_destination")]
    pub send_destination: Option<RuleMatch>,
    #[serde(rename = "@send_destination_prefix")]
    pub send_destination_prefix: Option<String>,
    #[serde(rename = "@send_type")]
    pub send_type: Option<RuleMatchType>,
    #[serde(rename = "@send_path")]
    pub send_path: Option<RuleMatch>,
    #[serde(rename = "@receive_interface")]
    pub receive_interface: Option<RuleMatch>,
    #[serde(rename = "@receive_member")]
    pub receive_member: Option<RuleMatch>,
    #[serde(rename = "@receive_error")]
    pub receive_error: Option<RuleMatch>,
    #[serde(rename = "@receive_sender")]
    pub receive_sender: Option<RuleMatch>,
    #[serde(rename = "@receive_type")]
    pub receive_type: Option<RuleMatchType>,
    #[serde(rename = "@receive_path")]
    pub receive_path: Option<RuleMatch>,
    #[serde(rename = "@send_requested_reply")]
    pub send_requested_reply: Option<bool>,
    #[serde(rename = "@receive_requested_reply")]
    pub receive_requested_reply: Option<bool>,
    #[serde(rename = "@eavesdrop")]
    pub eavesdrop: Option<bool>,
    #[serde(rename = "@own")]
    pub own: Option<RuleMatch>,
    #[serde(rename = "@own_prefix")]
    pub own_prefix: Option<String>,
    #[serde(rename = "@user")]
    pub user: Option<RuleMatch>,
    #[serde(rename = "@group")]
    pub group: Option<RuleMatch>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct RawSelinux {
    pub associate: Vec<Associate>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) struct RawTypeElement {
    #[serde(rename = "$text")]
    pub text: Type,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) struct RawUserElement {
    #[serde(rename = "$text")]
    pub text: Principal,
}
