#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BusType {
    #[default]
    Session,
    System,
}
