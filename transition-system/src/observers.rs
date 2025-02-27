use crate::book_state::BookState;
use crate::events::BookEvent;

/// Trait for state change observation
pub trait StateObserver {
    /// Called when a state transition occurs
    fn on_state_change(&self, from: &BookState, to: &BookState, event: &BookEvent);
}

/// Logs all transitions that occur in the system
#[derive(Debug)]
pub struct TransitionLogger;

impl StateObserver for TransitionLogger {
    fn on_state_change(&self, from: &BookState, to: &BookState, event: &BookEvent) {
        println!("LOGGER: Transition occurred: {from:?} --({event:?})--> {to:?}");
    }
}

/// Sends notifications for specific state transitions
#[derive(Debug)]
pub struct NotificationService;

impl StateObserver for NotificationService {
    fn on_state_change(&self, from: &BookState, to: &BookState, event: &BookEvent) {
        match (from, to, event) {
            (BookState::Reserved(_), BookState::CheckedOut(_), BookEvent::CheckOut(_)) => {
                println!("NOTIFICATION: Book has been checked out!");
            }
            (BookState::CheckedOut(_), BookState::Available, BookEvent::Return) => {
                println!("NOTIFICATION: Book has been returned!");
            }
            (BookState::UnderRepair, BookState::Available, BookEvent::CompleteRepair) => {
                println!("NOTIFICATION: Book has been repaired!");
            }
            _ => {}
        }
    }
}
