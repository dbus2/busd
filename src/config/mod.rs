use serde::Deserialize;

pub mod limits;
mod xml;

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
