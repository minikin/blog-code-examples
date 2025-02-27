//! Library state transition system for tracking book status in a library.
//!
//! This crate provides a state machine implementation for managing
//! library book states and transitions between them.

pub mod book_state;
pub mod events;
pub mod observers;
pub mod persistence;
pub mod system;
pub mod visualization;

pub use book_state::BookState;
pub use events::BookEvent;
pub use system::LibrarySystem;
pub use visualization::StateVisualization;
