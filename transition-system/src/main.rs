use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Read, Write},
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
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

        Self { seconds_since_epoch, nanos }
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
    states: Vec<BookState>,
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
        println!("LOGGER: Transition occurred: {:?} --({:?})--> {:?}", from, to, event);
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
    #[allow(dead_code)]
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

    fn add_transition(&mut self, from_state_idx: usize, event: BookEvent, to_state_idx: usize) {
        self.transitions.insert((from_state_idx, event), to_state_idx);
    }

    fn register_observer(&mut self, observer: Box<dyn StateObserver>) {
        self.observers.push(observer);
    }

    fn add_timing_constraint(
        &mut self,
        state_idx: usize,
        max_duration: Duration,
        timeout_event: BookEvent,
    ) {
        self.timing_constraints
            .insert(state_idx, TimingConstraints { max_duration, timeout_event });
    }

    fn check_timeout(&mut self) -> Option<BookEvent> {
        if let Some(constraint) = self.timing_constraints.get(&self.current_state_idx) {
            let time_in_state = Instant::now().duration_since(self.state_entry_time);
            if time_in_state > constraint.max_duration {
                return Some(constraint.timeout_event.clone());
            }
        }
        None
    }

    fn current_state(&self) -> &BookState {
        &self.states[self.current_state_idx]
    }

    fn process_event(&mut self, event: BookEvent) -> Result<&BookState, String> {
        // Check for timeouts first
        if let Some(timeout_event) = self.check_timeout() {
            println!("State timed out! Processing timeout event: {:?}", timeout_event);
            return self.process_event(timeout_event);
        }

        // Look up the transition
        match self.transitions.get(&(self.current_state_idx, event.clone())) {
            Some(&next_state_idx) => {
                let from_state = self.current_state().clone();

                // Apply the transition
                self.current_state_idx = next_state_idx;

                // Record the transition in history
                let now = Instant::now();
                let transition = StateTransition {
                    from: from_state.clone(),
                    to: self.current_state().clone(),
                    event: event.clone(),
                    timestamp: SerdeInstant::now(),
                    serializable_timestamp: SerializableInstant::from(now),
                };

                self.history.push(transition);

                // Maintain history size limit
                if self.history.len() > self.max_history_size {
                    self.history.pop();
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
                Err(format!(
                    "Cannot process event {:?} from current state {:?}",
                    event,
                    self.current_state()
                ))
            }
        }
    }

    #[allow(dead_code)]
    fn get_history(&self) -> &Vec<StateTransition> {
        &self.history
    }

    fn print_history(&self) {
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

    #[allow(dead_code)]
    fn save_state_to_file(&self) -> Result<(), Box<dyn std::error::Error>> {
        let serializable_state = SerializableSystemState {
            states: self.states.clone(),
            transitions: self
                .transitions
                .iter()
                .map(|((from, event), to)| ((*from, event.clone()), *to))
                .collect(),
            current_state_idx: self.current_state_idx,
            history: self.history.iter().cloned().collect(),
            max_history_size: self.max_history_size,
            timing_constraints: self
                .timing_constraints
                .iter()
                .map(|(state_idx, constraint)| (*state_idx, constraint.clone()))
                .collect(),
            system_id: self.system_id.clone(),
        };

        let serialized = serde_json::to_string_pretty(&serializable_state)?;

        let filename = format!("{}.json", self.system_id);
        println!("PERSISTENCE: Saving state to file: {}", filename);
        let mut file = File::create(&filename)?;
        file.write_all(serialized.as_bytes())?;

        Ok(())
    }

    fn load_state_from_file(system_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let filename = format!("{}.json", system_id);
        println!("PERSISTENCE: Loading state from file: {}", filename);

        // Read the file
        let mut file = File::open(&filename)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        // Deserialize the JSON
        let serializable_state: SerializableSystemState = serde_json::from_str(&contents)?;

        // Convert back to our runtime representation
        let mut system = LibrarySystem {
            states: serializable_state.states,
            transitions: serializable_state.transitions.into_iter().collect(),
            current_state_idx: serializable_state.current_state_idx,
            history: Vec::from(serializable_state.history),
            max_history_size: serializable_state.max_history_size,
            state_entry_time: Instant::now(), // Reset the entry time
            timing_constraints: serializable_state.timing_constraints.into_iter().collect(),
            observers: Vec::new(), // Observers need to be re-attached
            system_id: serializable_state.system_id,
        };

        // Re-register observers (these would normally be application-specific)
        system.register_observer(Box::new(TransitionLogger));
        system.register_observer(Box::new(NotificationService));

        Ok(system)
    }

    fn get_state_description(&self) -> String {
        match self.current_state() {
            BookState::Available => "Book is available for checkout".to_string(),
            BookState::Reserved(patron) => format!("Book is reserved by {}", patron),
            BookState::CheckedOut(patron) => format!("Book is checked out by {}", patron),
            BookState::InTransit => "Book is in transit between library branches".to_string(),
            BookState::UnderRepair => "Book is currently being repaired".to_string(),
            BookState::Lost => "Book is marked as lost".to_string(),
        }
    }
}

// Implementing display for nicer output
impl fmt::Display for LibrarySystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get_state_description())
    }
}

fn setup_library_system(system: &mut LibrarySystem) {
    // Set up all states
    let available_idx = 0; // Already added as initial
    let reserved_alice_idx = system.add_state(BookState::Reserved("Alice".to_string()));
    let checked_out_alice_idx = system.add_state(BookState::CheckedOut("Alice".to_string()));
    let reserved_bob_idx = system.add_state(BookState::Reserved("Bob".to_string()));
    let checked_out_bob_idx = system.add_state(BookState::CheckedOut("Bob".to_string()));
    let in_transit_idx = system.add_state(BookState::InTransit);
    let under_repair_idx = system.add_state(BookState::UnderRepair);
    let lost_idx = system.add_state(BookState::Lost);

    // Add transitions from Available state
    system.add_transition(
        available_idx,
        BookEvent::Reserve("Alice".to_string()),
        reserved_alice_idx,
    );
    system.add_transition(available_idx, BookEvent::Reserve("Bob".to_string()), reserved_bob_idx);
    system.add_transition(
        available_idx,
        BookEvent::CheckOut("Alice".to_string()),
        checked_out_alice_idx,
    );
    system.add_transition(
        available_idx,
        BookEvent::CheckOut("Bob".to_string()),
        checked_out_bob_idx,
    );
    system.add_transition(available_idx, BookEvent::Transfer, in_transit_idx);
    system.add_transition(available_idx, BookEvent::SendToRepair, under_repair_idx);
    system.add_transition(available_idx, BookEvent::ReportLost, lost_idx);

    // Add transitions from Reserved states
    system.add_transition(reserved_alice_idx, BookEvent::CancelReservation, available_idx);
    system.add_transition(
        reserved_alice_idx,
        BookEvent::CheckOut("Alice".to_string()),
        checked_out_alice_idx,
    );
    system.add_transition(reserved_alice_idx, BookEvent::ReportLost, lost_idx);

    system.add_transition(reserved_bob_idx, BookEvent::CancelReservation, available_idx);
    system.add_transition(
        reserved_bob_idx,
        BookEvent::CheckOut("Bob".to_string()),
        checked_out_bob_idx,
    );
    system.add_transition(reserved_bob_idx, BookEvent::ReportLost, lost_idx);

    // Add transitions from CheckedOut states
    system.add_transition(checked_out_alice_idx, BookEvent::Return, available_idx);
    system.add_transition(checked_out_alice_idx, BookEvent::ReportLost, lost_idx);

    system.add_transition(checked_out_bob_idx, BookEvent::Return, available_idx);
    system.add_transition(checked_out_bob_idx, BookEvent::ReportLost, lost_idx);

    // Add transitions from InTransit state
    system.add_transition(in_transit_idx, BookEvent::TransferComplete, available_idx);
    system.add_transition(in_transit_idx, BookEvent::ReportLost, lost_idx);

    // Add transitions from UnderRepair state
    system.add_transition(under_repair_idx, BookEvent::CompleteRepair, available_idx);
    system.add_transition(under_repair_idx, BookEvent::ReportLost, lost_idx);

    // Add transitions from Lost state
    system.add_transition(lost_idx, BookEvent::Found, available_idx);

    // Add timing constraints
    // Books can only be reserved for 3 days
    system.add_timing_constraint(
        reserved_alice_idx,
        Duration::from_secs(3 * 24 * 60 * 60), // 3 days
        BookEvent::CancelReservation,
    );
    system.add_timing_constraint(
        reserved_bob_idx,
        Duration::from_secs(3 * 24 * 60 * 60), // 3 days
        BookEvent::CancelReservation,
    );

    // Books can be checked out for 14 days
    system.add_timing_constraint(
        checked_out_alice_idx,
        Duration::from_secs(14 * 24 * 60 * 60), // 14 days
        BookEvent::Return,
    );
    system.add_timing_constraint(
        checked_out_bob_idx,
        Duration::from_secs(14 * 24 * 60 * 60), // 14 days
        BookEvent::Return,
    );
}

fn main() {
    // Create a new library system with a book that's initially available
    let mut book_system = LibrarySystem::new(BookState::Available, "book-1234");

    // Register observers
    book_system.register_observer(Box::new(TransitionLogger));
    book_system.register_observer(Box::new(NotificationService));

    // Set up all the states and transitions
    setup_library_system(&mut book_system);

    // Simulate book lifecycle with transition system
    println!("Initial state: {}", book_system);

    // Alice reserves the book
    match book_system.process_event(BookEvent::Reserve("Alice".to_string())) {
        Ok(_) => println!("New state: {}", book_system),
        Err(e) => println!("Error: {}", e),
    }

    // Alice checks out the book
    match book_system.process_event(BookEvent::CheckOut("Alice".to_string())) {
        Ok(_) => println!("New state: {}", book_system),
        Err(e) => println!("Error: {}", e),
    }

    // Alice returns the book
    match book_system.process_event(BookEvent::Return) {
        Ok(_) => println!("New state: {}", book_system),
        Err(e) => println!("Error: {}", e),
    }

    // Print the transition history
    book_system.print_history();

    // In a real application, we would have a completely separate run here
    // To simulate this, we'll load the state from the file
    println!("\n--- Simulating application restart ---\n");

    match LibrarySystem::load_state_from_file("book-1234") {
        Ok(mut loaded_system) => {
            println!("Successfully loaded system from file: {}", loaded_system);

            // Continue working with the loaded system
            match loaded_system.process_event(BookEvent::SendToRepair) {
                Ok(_) => println!("New state after loading: {}", loaded_system),
                Err(e) => println!("Error after loading: {}", e),
            }

            // Print the history (should include previous transitions too)
            loaded_system.print_history();
        }
        Err(e) => {
            println!("Failed to load system: {}", e);
        }
    }
}
