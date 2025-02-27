use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    book_state::BookState,
    events::BookEvent,
    observers::{NotificationService, StateObserver, TransitionLogger},
    persistence::SerializableInstant,
};

/// Custom error type for library system operations
#[derive(Debug)]
pub enum LibraryError {
    /// The requested transition is not valid for the current state
    InvalidTransition { from_state: BookState, event: BookEvent },
    /// Error occurred while saving state
    PersistenceError(String),
    /// Error occurred while loading state
    LoadError(String),
}

impl std::error::Error for LibraryError {}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from_state, event } => {
                write!(f, "Cannot process event {event:?} from current state {from_state:?}")
            }
            Self::PersistenceError(msg) => write!(f, "Persistence error: {msg}"),
            Self::LoadError(msg) => write!(f, "Load error: {msg}"),
        }
    }
}

/// Represents a state transition in the system
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateTransition {
    /// The state before the transition
    pub from: BookState,
    /// The state after the transition
    pub to: BookState,
    /// The event that triggered the transition
    pub event: BookEvent,
    /// When the transition occurred
    pub timestamp: SerializableInstant,
}

/// Timing constraints for state transitions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimingConstraints {
    /// Maximum time allowed in a state
    pub max_duration: Duration,
    /// Event to trigger when timeout occurs
    pub timeout_event: BookEvent,
}

/// Serializable representation of the system state
#[derive(Debug, Deserialize, Serialize)]
struct SerializableSystemState {
    /// Collection of all book states
    states: Vec<BookState>,
    /// Mapping of state transitions
    transitions: Vec<((usize, BookEvent), usize)>,
    /// Index of the current state
    current_state_idx: usize,
    /// Record of state transition history
    history: Vec<StateTransition>,
    /// Maximum number of history entries to keep
    max_history_size: usize,
    /// State timing constraints
    timing_constraints: Vec<(usize, TimingConstraints)>,
    /// Unique identifier for this system
    system_id: String,
}

/// Library book state machine
pub struct LibrarySystem {
    /// Collection of all book states
    states: Vec<BookState>,
    /// Mapping of state transitions
    transitions: HashMap<(usize, BookEvent), usize>,
    /// Index of the current state
    current_state_idx: usize,
    /// Record of state transition history
    history: Vec<StateTransition>,
    /// Maximum number of history entries to keep
    max_history_size: usize,
    /// When the current state was entered
    state_entry_time: Instant,
    /// State timing constraints
    timing_constraints: HashMap<usize, TimingConstraints>,
    /// Registered state change observers
    observers: Vec<Box<dyn StateObserver>>,
    /// Unique identifier for this system
    system_id: String,
}

// Manual implementation of Debug for LibrarySystem
impl fmt::Debug for LibrarySystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibrarySystem")
            .field("states", &self.states)
            .field("transitions", &self.transitions)
            .field("current_state_idx", &self.current_state_idx)
            .field("history", &self.history)
            .field("max_history_size", &self.max_history_size)
            .field("state_entry_time", &self.state_entry_time)
            .field("timing_constraints", &self.timing_constraints)
            .field("observers_count", &self.observers.len())
            .field("system_id", &self.system_id)
            .finish()
    }
}

impl LibrarySystem {
    /// Create a new library system with the specified initial state
    #[must_use]
    pub fn new(initial_state: BookState, system_id: &str) -> Self {
        Self {
            states: vec![initial_state],
            transitions: HashMap::new(),
            current_state_idx: 0,
            history: Vec::new(),
            max_history_size: 100,
            state_entry_time: Instant::now(),
            timing_constraints: HashMap::new(),
            observers: Vec::new(),
            system_id: system_id.to_string(),
        }
    }

    /// Add a state to the system, or return its index if it already exists
    #[allow(clippy::arithmetic_side_effects)]
    pub fn add_state(&mut self, state: BookState) -> usize {
        if let Some(pos) = self.states.iter().position(|s| *s == state) {
            pos
        } else {
            self.states.push(state);
            self.states.len() - 1
        }
    }

    /// Define a valid transition from one state to another when an event occurs
    pub fn add_transition(&mut self, from_state_idx: usize, event: BookEvent, to_state_idx: usize) {
        self.transitions.insert((from_state_idx, event), to_state_idx);
    }

    /// Register an observer to be notified of state changes
    pub fn register_observer(&mut self, observer: Box<dyn StateObserver>) {
        self.observers.push(observer);
    }

    /// Add a timing constraint to a state
    pub fn add_timing_constraint(
        &mut self,
        state_idx: usize,
        max_duration: Duration,
        timeout_event: BookEvent,
    ) {
        self.timing_constraints
            .insert(state_idx, TimingConstraints { max_duration, timeout_event });
    }

    /// Check if the current state has timed out
    fn check_timeout(&mut self) -> Option<BookEvent> {
        if let Some(constraint) = self.timing_constraints.get(&self.current_state_idx) {
            let time_in_state = Instant::now().duration_since(self.state_entry_time);
            if time_in_state > constraint.max_duration {
                return Some(constraint.timeout_event.clone());
            }
        }
        None
    }

    /// Get the current state of the system
    ///
    /// # Panics
    ///
    /// Panics if the current state index is invalid, which should never happen
    /// during normal operation and would indicate a bug in the library.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn current_state(&self) -> &BookState {
        self.states.get(self.current_state_idx).expect("Invalid current state index")
    }

    /// Process an event, potentially changing the system state
    ///
    /// # Errors
    ///
    /// Returns a `LibraryError::InvalidTransition` if the event cannot be processed
    /// from the current state because no valid transition is defined
    pub fn process_event(&mut self, event: BookEvent) -> Result<&BookState, LibraryError> {
        // Check for timeouts first
        if let Some(timeout_event) = self.check_timeout() {
            println!("State timed out! Processing timeout event: {timeout_event:?}");
            return self.process_event(timeout_event);
        }

        // Look up the transition
        let from_state = self.current_state().clone();

        match self.transitions.get(&(self.current_state_idx, event.clone())) {
            Some(&next_state_idx) => {
                // Apply the transition
                self.current_state_idx = next_state_idx;

                // Record the transition in history
                let transition = StateTransition {
                    from: from_state.clone(),
                    to: self.current_state().clone(),
                    event: event.clone(),
                    timestamp: SerializableInstant::now(),
                };

                self.history.push(transition);

                // Maintain history size limit
                if self.history.len() > self.max_history_size {
                    self.history.remove(0); // Remove oldest entry
                }

                // Reset state entry time for timing constraints
                self.state_entry_time = Instant::now();

                // Notify observers
                for observer in &self.observers {
                    observer.on_state_change(&from_state, self.current_state(), &event);
                }

                Ok(self.current_state())
            }
            None => {
                // No valid transition for this event from current state
                Err(LibraryError::InvalidTransition { from_state, event })
            }
        }
    }

    /// Get the complete transition history
    #[must_use]
    pub fn get_history(&self) -> &Vec<StateTransition> {
        &self.history
    }

    /// Print the transition history to stdout
    #[allow(clippy::arithmetic_side_effects)]
    pub fn print_history(&self) {
        println!("Transition History:");
        for (i, transition) in self.history.iter().enumerate() {
            println!(
                "{}. {:?} --({:?})--> {:?}",
                i + 1,
                transition.from,
                transition.event,
                transition.to
            );
        }
    }

    /// Save the system state to a JSON file
    ///
    /// # Errors
    ///
    /// Returns a `LibraryError::PersistenceError` if:
    /// - The state cannot be serialized to JSON
    /// - The file cannot be created
    /// - The data cannot be written to the file
    pub fn save_state_to_file(&self) -> Result<(), LibraryError> {
        let serializable_state = SerializableSystemState {
            states: self.states.clone(),
            transitions: self
                .transitions
                .iter()
                .map(|((from, event), to)| ((*from, event.clone()), *to))
                .collect(),
            current_state_idx: self.current_state_idx,
            history: self.history.clone(),
            max_history_size: self.max_history_size,
            timing_constraints: self
                .timing_constraints
                .iter()
                .map(|(state_idx, constraint)| (*state_idx, constraint.clone()))
                .collect(),
            system_id: self.system_id.clone(),
        };

        let serialized = serde_json::to_string_pretty(&serializable_state)
            .map_err(|e| LibraryError::PersistenceError(e.to_string()))?;

        let system_id = &self.system_id;
        let filename = format!("{system_id}.json");
        println!("PERSISTENCE: Saving state to file: {filename}");

        let mut file = File::create(&filename)
            .map_err(|e| LibraryError::PersistenceError(format!("Failed to create file: {e}")))?;

        file.write_all(serialized.as_bytes())
            .map_err(|e| LibraryError::PersistenceError(format!("Failed to write to file: {e}")))?;

        Ok(())
    }

    /// Load the system state from a JSON file
    ///
    /// # Errors
    ///
    /// Returns a `LibraryError::LoadError` if:
    /// - The file does not exist
    /// - The file cannot be opened
    /// - The file cannot be read
    /// - The JSON parsing fails
    pub fn load_state_from_file(system_id: &str) -> Result<Self, LibraryError> {
        let filename = format!("{system_id}.json");
        println!("PERSISTENCE: Loading state from file: {filename}");

        if !Path::new(&filename).exists() {
            return Err(LibraryError::LoadError(format!("File does not exist: {filename}")));
        }

        // Read the file
        let mut file = File::open(&filename)
            .map_err(|e| LibraryError::LoadError(format!("Failed to open file: {e}")))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| LibraryError::LoadError(format!("Failed to read file: {e}")))?;

        // Deserialize the JSON
        let serializable_state: SerializableSystemState = serde_json::from_str(&contents)
            .map_err(|e| LibraryError::LoadError(format!("Failed to parse JSON: {e}")))?;

        // Convert back to our runtime representation
        let mut system = Self {
            states: serializable_state.states,
            transitions: serializable_state.transitions.into_iter().collect(),
            current_state_idx: serializable_state.current_state_idx,
            history: serializable_state.history,
            max_history_size: serializable_state.max_history_size,
            state_entry_time: Instant::now(), // Reset the entry time
            timing_constraints: serializable_state.timing_constraints.into_iter().collect(),
            observers: Vec::new(), // Observers need to be re-attached
            system_id: serializable_state.system_id,
        };

        // Re-register standard observers
        system.register_observer(Box::new(TransitionLogger));
        system.register_observer(Box::new(NotificationService));

        Ok(system)
    }

    /// Get all states in the system
    #[must_use]
    pub fn get_states(&self) -> &Vec<BookState> {
        &self.states
    }

    /// Get the index of the current state
    #[must_use]
    pub fn get_current_state_idx(&self) -> usize {
        self.current_state_idx
    }

    /// Get all transitions defined in the system
    #[must_use]
    pub fn get_all_transitions(&self) -> &HashMap<(usize, BookEvent), usize> {
        &self.transitions
    }

    /// Get all timing constraints defined in the system
    #[must_use]
    pub fn get_timing_constraints(&self) -> &HashMap<usize, TimingConstraints> {
        &self.timing_constraints
    }

    /// Find the index of a state in the system
    #[must_use]
    pub fn get_state_idx(&self, state: &BookState) -> Option<usize> {
        self.states.iter().position(|s| s == state)
    }
}

// Implementing display for nicer output
impl fmt::Display for LibrarySystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.current_state().get_description())
    }
}

// Include tests module
#[cfg(test)]
mod tests;
