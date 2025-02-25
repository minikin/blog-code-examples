use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
enum BookState {
    #[default]
    Available,
    Reserved(String),
    CheckedOut(String),
    InTransit,
    UnderRepair,
    Lost,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
enum BookEvent {
    Reserve(String),
    CancelReservation,
    CheckOut(String),
    Return,
    SendToRepair,
    CompleteRepair,
    Transfer,
    TransferComplete,
    ReportLost,
    #[default]
    Found,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]

struct SerializableInstant {
    seconds_since_epoch: u64,
    nanos: u32,
}

impl From<Instant> for SerializableInstant {
    fn from(instant: Instant) -> Self {
        let duration = instant.elapsed();
        let now = Instant::now();

        let seconds_since_epoch = now.duration_since(Instant::now()).as_secs();

        let nanos = duration.subsec_nanos();

        Self {
            seconds_since_epoch,
            nanos,
        }
    }
}

// A wrapper around Instant that implements serialization
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SerdeInstant(Instant);

impl SerdeInstant {
    fn now() -> Self {
        SerdeInstant(Instant::now())
    }

    #[allow(dead_code)]
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

impl Serialize for SerdeInstant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as null or some other placeholder
        serializer.serialize_unit()
    }
}

impl<'de> Deserialize<'de> for SerdeInstant {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the placeholder (ignoring the actual value)
        let _ = serde::de::IgnoredAny::deserialize(deserializer)?;

        // Return a new instance
        Ok(SerdeInstant::now())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StateTransition {
    from: BookState,
    to: BookState,
    event: BookEvent,
    timestamp: SerdeInstant,
    serializable_timestamp: SerializableInstant,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TimingConstraints {
    max_duration: Duration,
    timeout_event: BookEvent,
}

#[derive(Debug, Deserialize, Serialize)]
struct SerializableSystemState {
    state: Vec<BookState>,
    transitions: Vec<((usize, BookEvent), usize)>,
    current_state_idx: usize,
    history: Vec<StateTransition>,
    max_history_size: usize,
    timing_constraints: Vec<(usize, TimingConstraints)>,
    system_id: String,
}

trait StateObserver {
    fn on_state_change(&self, from: &BookState, to: &BookState, event: &BookEvent);
}

// Example Observers
struct TransitionLogger;
struct NotificationService;

impl StateObserver for TransitionLogger {
    fn on_state_change(&self, from: &BookState, to: &BookState, event: &BookEvent) {
        println!(
            "LOGGER: Transition occurred: {:?} --({:?})--> {:?}",
            from, to, event
        );
    }
}

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

struct LibrarySystem {
    states: Vec<BookState>,
    transitions: HashMap<(usize, BookEvent), usize>,
    current_state_idx: usize,
    history: Vec<StateTransition>,
    max_history_size: usize,
    state_entry_time: Instant,
    timing_constraints: HashMap<usize, TimingConstraints>,
    observers: Vec<Box<dyn StateObserver>>,
    system_id: String,
}

impl LibrarySystem {
    fn new(initial_state: BookState, system_id: &str) -> Self {
        LibrarySystem {
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

    fn add_state(&mut self, state: BookState) -> usize {
        if let Some(pos) = self.states.iter().position(|s| *s == state) {
            pos
        } else {
            self.states.push(state);
            self.states.len() - 1
        }
    }
}

fn main() {
    println!("Hello, world!");
}
