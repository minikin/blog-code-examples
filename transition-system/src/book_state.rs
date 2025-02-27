use serde::{Deserialize, Serialize};

/// Represents the possible states of a library book
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum BookState {
    /// Book is available for checkout
    #[default]
    Available,
    /// Book is reserved by a patron
    Reserved(String),
    /// Book is checked out by a patron
    CheckedOut(String),
    /// Book is in transit between library branches
    InTransit,
    /// Book is being repaired
    UnderRepair,
    /// Book is marked as lost
    Lost,
}

impl BookState {
    /// Get a human-readable description of the current state
    #[must_use]
    pub fn get_description(&self) -> String {
        match self {
            Self::Available => "Book is available for checkout".to_string(),
            Self::Reserved(patron) => format!("Book is reserved by {patron}"),
            Self::CheckedOut(patron) => format!("Book is checked out by {patron}"),
            Self::InTransit => "Book is in transit between library branches".to_string(),
            Self::UnderRepair => "Book is currently being repaired".to_string(),
            Self::Lost => "Book is marked as lost".to_string(),
        }
    }
}
