use serde::Deserialize;

use super::{
    raw::{RawPolicy, RawPolicyContext, RawRule, RawRuleAttributes},
    Error, Principal,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConnectRule {
    pub group: Option<RuleMatch>,
    pub user: Option<RuleMatch>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnRule {
    pub own: Option<RuleMatch>,
    pub own_prefix: Option<String>,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReceiveRule {
    pub eavesdrop: Option<bool>,
    pub receive_error: Option<RuleMatch>,
    pub receive_interface: Option<RuleMatch>,
    pub receive_member: Option<RuleMatch>,
    pub receive_path: Option<RuleMatch>,
    pub receive_requested_reply: Option<bool>,
    pub receive_sender: Option<RuleMatch>,
    pub receive_type: Option<RuleMatchType>,
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
    pub eavesdrop: Option<bool>,
    pub send_broadcast: Option<bool>,
    pub send_destination: Option<RuleMatch>,
    pub send_destination_prefix: Option<String>,
    pub send_error: Option<RuleMatch>,
    pub send_interface: Option<RuleMatch>,
    pub send_member: Option<RuleMatch>,
    pub send_path: Option<RuleMatch>,
    pub send_requested_reply: Option<bool>,
    pub send_type: Option<RuleMatchType>,
}
