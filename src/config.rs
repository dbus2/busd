use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename = "busconfig", rename_all = "snake_case")]
pub struct BusConfig {
    #[serde(rename = "$value", default)]
    pub elements: Vec<Element>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    User(String),
    Type(String),
    Fork,
    Syslog,
    KeepUmask,
    Listen(String),
    #[serde(rename = "pidfile")]
    PIDFile(String),
    #[serde(rename = "includedir")]
    IncludeDir(String),
    #[serde(rename = "standard_session_servicedirs")]
    StandardSessionServiceDirs,
    #[serde(rename = "standard_system_servicedirs")]
    StandardSystemServiceDirs,
    #[serde(rename = "servicedir")]
    ServiceDir(String),
    #[serde(rename = "servicehelper")]
    ServiceHelper(String),
    Auth(String),
    Include(Include),
    Policy(Policy),
    Limit(Limit),
    #[serde(rename = "selinux")]
    SELinux(SELinux),
    #[serde(rename = "apparmor")]
    AppArmor(AppArmor),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Include {
    #[serde(rename = "$value")]
    pub content: String,
    #[serde(rename = "@ignore_missing", deserialize_with = "de_yes_no", default)]
    pub ignore_missing: Option<bool>,
    #[serde(
        rename = "@if_selinux_enabled",
        deserialize_with = "de_yes_no",
        default
    )]
    pub if_selinux_enabled: Option<bool>,
    #[serde(
        rename = "@selinux_root_relative",
        deserialize_with = "de_yes_no",
        default
    )]
    pub selinux_root_relative: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Policy {
    #[serde(rename = "$value")]
    pub rules: Vec<Rule>,
    #[serde(rename = "@context")]
    pub context: Option<PolicyContext>,
    #[serde(rename = "@user")]
    pub user: Option<String>,
    #[serde(rename = "@group")]
    pub group: Option<String>,
    #[serde(
        rename = "@at_console",
        deserialize_with = "de_yes_no_true_false",
        default
    )]
    pub at_console: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyContext {
    Default,
    Mandatory,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    #[serde(deserialize_with = "de_allow_deny")]
    Allow(AllowDeny),
    Deny(AllowDeny),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AllowDeny {
    #[serde(rename = "@send_interface")]
    pub send_interface: Option<String>,
    #[serde(rename = "@send_member")]
    pub send_member: Option<String>,
    #[serde(rename = "@send_error")]
    pub send_error: Option<String>,
    #[serde(rename = "@send_destination")]
    pub send_destination: Option<String>,
    #[serde(rename = "@send_path")]
    pub send_path: Option<String>,
    #[serde(rename = "@send_type")]
    pub send_type: Option<String>,
    #[serde(
        rename = "@send_requested_reply",
        deserialize_with = "de_true_false",
        default
    )]
    pub send_requested_reply: Option<bool>,
    #[serde(
        rename = "@send_broadcast",
        deserialize_with = "de_true_false",
        default
    )]
    pub send_broadcast: Option<bool>,
    #[serde(rename = "@receive_interface")]
    pub receive_interface: Option<String>,
    #[serde(rename = "@receive_member")]
    pub receive_member: Option<String>,
    #[serde(rename = "@receive_error")]
    pub receive_error: Option<String>,
    #[serde(rename = "@receive_sender")]
    pub receive_sender: Option<String>,
    #[serde(rename = "@receive_path")]
    pub receive_path: Option<String>,
    #[serde(rename = "@receive_type")]
    pub receive_type: Option<String>,
    #[serde(
        rename = "@receive_requested_reply",
        deserialize_with = "de_true_false",
        default
    )]
    pub receive_requested_reply: Option<bool>,
    #[serde(rename = "@eavesdrop", deserialize_with = "de_true_false", default)]
    pub eavesdrop: Option<bool>,
    #[serde(rename = "@min_fds")]
    pub min_fds: Option<String>,
    #[serde(rename = "@max_fds")]
    pub max_fds: Option<String>,
    #[serde(rename = "@own")]
    pub own: Option<String>,
    #[serde(rename = "@own_prefix")]
    pub own_prefix: Option<String>,
    #[serde(rename = "@user")]
    pub user: Option<String>,
    #[serde(rename = "@group")]
    pub group: Option<String>,
    #[serde(rename = "@log", deserialize_with = "de_true_false", default)]
    pub log: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Limit {
    #[serde(rename = "$value", deserialize_with = "de_limit")]
    pub value: usize,
    #[serde(rename = "@name")]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SELinux {
    #[serde(rename = "$value")]
    pub associates: Vec<Associate>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename = "associate")]
pub struct Associate {
    #[serde(rename = "@own")]
    pub own: String,
    #[serde(rename = "@context")]
    pub context: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AppArmor {
    mode: AppArmorMode,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppArmorMode {
    Required,
    Enabled,
    Diabled,
}

impl BusConfig {
    /// Write the XML document to writer.
    pub fn to_writer<W: Write>(&self, writer: W) -> Result<(), anyhow::Error> {
        Ok(quick_xml::se::to_writer(writer, self)?)
    }

    /// Read a config from the filesystem, with <include*> support.
    pub fn read<P>(path: P) -> Result<Self, anyhow::Error>
    where
        P: AsRef<Path>,
    {
        use std::collections::HashSet;

        fn join_paths<P>(parent: Option<&Path>, path: P) -> PathBuf
        where
            P: AsRef<Path>,
        {
            let path = path.as_ref();
            match parent {
                Some(p) => p.join(path),
                None => path.to_path_buf(),
            }
        }

        pub fn read_visited<P>(
            path: P,
            visited: &mut HashSet<PathBuf>,
        ) -> Result<BusConfig, anyhow::Error>
        where
            P: AsRef<Path>,
        {
            let path: &Path = path.as_ref();
            if !visited.insert(path.to_owned()) {
                bail!("File {} visited recursively", path.display());
            }
            let file = std::fs::File::open(path)
                .with_context(|| format!("Failed to read from {}", path.display()))?;
            let reader = std::io::BufReader::new(file);
            let parent = path.parent();
            let config: BusConfig = quick_xml::de::from_reader(reader)?;
            let mut elements = Vec::new();
            for e in config.elements {
                match e {
                    Element::Include(inc) => {
                        if inc.if_selinux_enabled.unwrap_or(false) {
                            // FIXME: add selinux support
                            continue;
                        }

                        let p = join_paths(parent, inc.content);
                        match read_visited(p, visited) {
                            Ok(mut config) => {
                                elements.append(&mut config.elements);
                            }
                            Err(e) => {
                                if let (Some(true), Some(e)) =
                                    (inc.ignore_missing, e.downcast_ref::<std::io::Error>())
                                {
                                    if e.kind() == std::io::ErrorKind::NotFound {
                                        continue;
                                    }
                                }
                                return Err(e);
                            }
                        }
                    }
                    Element::IncludeDir(inc) => {
                        let path = join_paths(parent, inc);
                        let Ok(entries) = std::fs::read_dir(path) else {
                            continue;
                        };
                        for entry in entries {
                            let mut config = read_visited(entry?.path(), visited)?;
                            elements.append(&mut config.elements);
                        }
                    }
                    _ => elements.push(e),
                }
            }
            Ok(BusConfig { elements })
        }

        let mut visited = HashSet::new();
        read_visited(path, &mut visited)
    }
}

impl<'a> TryFrom<&'a str> for BusConfig {
    type Error = anyhow::Error;

    fn try_from(s: &'a str) -> Result<BusConfig, anyhow::Error> {
        Ok(quick_xml::de::from_str(s)?)
    }
}

impl TryFrom<&BusConfig> for String {
    type Error = anyhow::Error;

    fn try_from(c: &BusConfig) -> Result<String, anyhow::Error> {
        let mut writer = String::new();
        c.to_writer(&mut writer)?;
        Ok(writer)
    }
}

fn de_limit<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = usize::deserialize(deserializer)?;
    if v > i32::MAX as _ {
        Err(serde::de::Error::custom("Limit is > i32::MAX"))
    } else {
        Ok(v)
    }
}

fn de_allow_deny<'de, D>(deserializer: D) -> Result<AllowDeny, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = AllowDeny::deserialize(deserializer)?;
    let has_send = v.send_interface.is_some()
        || v.send_member.is_some()
        || v.send_error.is_some()
        || v.send_destination.is_some()
        || v.send_path.is_some()
        || v.send_type.is_some()
        || v.send_requested_reply.is_some()
        || v.send_broadcast.is_some();

    let has_recv = v.receive_interface.is_some()
        || v.receive_member.is_some()
        || v.receive_error.is_some()
        || v.receive_sender.is_some()
        || v.receive_path.is_some()
        || v.receive_type.is_some()
        || v.receive_requested_reply.is_some();

    if !has_send
        && !has_recv
        && v.eavesdrop.is_none()
        && v.min_fds.is_none()
        && v.max_fds.is_none()
        && v.own.is_none()
        && v.own_prefix.is_none()
        && v.user.is_none()
        && v.group.is_none()
        && v.log.is_none()
    {
        return Err(serde::de::Error::custom("No limit attribute set"));
    }

    if has_send && has_recv {
        return Err(serde::de::Error::custom(
            "send and receive attributes cannot be combined",
        ));
    }

    Ok(v)
}

fn de_yes_no<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = String::deserialize(deserializer)?;
    match v.as_str() {
        "yes" => Ok(Some(true)),
        "no" => Ok(Some(false)),
        _ => Err(serde::de::Error::custom("Invalid boolean")),
    }
}

fn de_true_false<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = String::deserialize(deserializer)?;
    match v.as_str() {
        "true" => Ok(Some(true)),
        "false" => Ok(Some(false)),
        _ => Err(serde::de::Error::custom("Invalid boolean")),
    }
}

fn de_yes_no_true_false<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = String::deserialize(deserializer)?;
    match v.as_str() {
        "yes" | "true" => Ok(Some(true)),
        "no" | "false" => Ok(Some(false)),
        _ => Err(serde::de::Error::custom("Invalid boolean")),
    }
}
