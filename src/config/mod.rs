use serde::Deserialize;

pub mod limits;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BusType {
    Session,
    System,
}
