use serde::{Deserialize, Serialize};

/// Events that can cause a book state transition
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum BookEvent {
    /// Reserve a book for a patron
    Reserve(String),
    /// Cancel a reservation
    CancelReservation,
    /// Check out a book to a patron
    CheckOut(String),
    /// Return a book to the library
    Return,
    /// Send a book for repair
    SendToRepair,
    /// Mark a book as repaired
    CompleteRepair,
    /// Transfer a book to another branch
    Transfer,
    /// Mark a transfer as complete
    TransferComplete,
    /// Report a book as lost
    ReportLost,
    /// Book has been found
    #[default]
    Found,
}
