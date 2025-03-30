// Transition System in Rust
// Ready for Rust Playground: https://play.rust-lang.org/?version=stable&mode=debug&edition=2024

use std::cell::RefCell;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::rc::Rc;

/// Trait for types that can be used as states in a transition system.
pub trait State: Clone + Debug + PartialEq {}

/// Automatically implement State for any type that satisfies the required bounds.
impl<T: Clone + Debug + PartialEq> State for T {}

/// Trait for types that define transitions between states.
pub trait Transition<S: State> {
    /// The event type that triggers this transition.
    type Event;

    /// The error type that may be returned if a transition fails.
    type Error;

    /// Apply a transition based on the current state and an event.
    /// Returns either the new state or an error if the transition is invalid.
    fn apply(&self, state: &S, event: Self::Event) -> Result<S, Self::Error>;

    /// Check if a transition is valid for the current state and event
    /// without actually performing the transition.
    fn is_valid(&self, state: &S, event: &Self::Event) -> bool;
}

/// A typed transition that can only be applied to specific source and target states.
pub struct TypedTransition<S, E, Src, Tgt, F>
where
    S: State,
    Src: 'static,
    Tgt: 'static,
    F: Fn(&S, E) -> Result<S, TransitionError>,
{
    transition_fn: F,
    _source_state: PhantomData<Src>,
    _target_state: PhantomData<Tgt>,
    _state: PhantomData<S>,
    _event: PhantomData<E>,
}

/// Possible errors during state transitions.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionError {
    /// The transition is not allowed from the current state.
    InvalidTransition,
    /// A guard condition prevented the transition.
    GuardFailed(String),
    /// A custom error occurred during the transition.
    Custom(String),
}

impl<S, E, Src, Tgt, F> TypedTransition<S, E, Src, Tgt, F>
where
    S: State,
    Src: 'static,
    Tgt: 'static,
    F: Fn(&S, E) -> Result<S, TransitionError>,
{
    /// Create a new typed transition with the provided transition function.
    pub fn new(transition_fn: F) -> Self {
        Self {
            transition_fn,
            _source_state: PhantomData,
            _target_state: PhantomData,
            _state: PhantomData,
            _event: PhantomData,
        }
    }
}

impl<S, E, Src, Tgt, F> Transition<S> for TypedTransition<S, E, Src, Tgt, F>
where
    S: State,
    E: Clone,
    Src: 'static,
    Tgt: 'static,
    F: Fn(&S, E) -> Result<S, TransitionError>,
{
    type Event = E;
    type Error = TransitionError;

    fn apply(&self, state: &S, event: Self::Event) -> Result<S, Self::Error> {
        (self.transition_fn)(state, event)
    }

    fn is_valid(&self, state: &S, event: &Self::Event) -> bool {
        match (self.transition_fn)(state, event.clone()) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

/// A transition system that manages states and transitions.
pub struct TransitionSystem<S, E>
where
    S: State,
{
    current_state: S,
    transitions: Vec<Box<dyn Transition<S, Event = E, Error = TransitionError>>>,
}

impl<S, E> TransitionSystem<S, E>
where
    S: State,
    E: Clone,
{
    /// Create a new transition system with the given initial state.
    pub fn new(initial_state: S) -> Self {
        Self { current_state: initial_state, transitions: Vec::new() }
    }

    /// Register a transition in the system.
    pub fn register_transition<T>(&mut self, transition: T)
    where
        T: Transition<S, Event = E, Error = TransitionError> + 'static,
    {
        self.transitions.push(Box::new(transition));
    }

    /// Apply an event to trigger a state transition.
    pub fn apply_event(&mut self, event: E) -> Result<&S, TransitionError> {
        for transition in &self.transitions {
            if transition.is_valid(&self.current_state, &event) {
                match transition.apply(&self.current_state, event.clone()) {
                    Ok(new_state) => {
                        self.current_state = new_state;
                        return Ok(&self.current_state);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Err(TransitionError::InvalidTransition)
    }

    /// Get the current state of the system.
    pub fn current_state(&self) -> &S {
        &self.current_state
    }

    /// Check if a transition is possible from the current state.
    pub fn can_transition(&self, event: &E) -> bool {
        self.transitions.iter().any(|t| t.is_valid(&self.current_state, event))
    }

    /// Get all possible transitions from the current state.
    pub fn possible_transitions(&self, events: &[E]) -> Vec<E>
    where
        E: Clone,
    {
        events.iter().filter(|e| self.can_transition(e)).cloned().collect()
    }
}

/// A builder for creating typed transitions with guards and actions.
pub struct TransitionBuilder<S, E>
where
    S: State,
{
    source_states: Vec<S>,
    target_state: Option<S>,
    event: Option<E>,
    guards: Vec<Box<dyn Fn(&S, &E) -> Result<(), String>>>,
    actions: Vec<Box<dyn FnMut(&S, &E)>>,
}

impl<S, E> TransitionBuilder<S, E>
where
    S: State,
    E: Clone,
{
    /// Create a new transition builder.
    pub fn new() -> Self {
        Self {
            source_states: Vec::new(),
            target_state: None,
            event: None,
            guards: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Set the source state for this transition.
    pub fn from(mut self, state: S) -> Self {
        self.source_states.push(state);
        self
    }

    /// Set the target state for this transition.
    pub fn to(mut self, state: S) -> Self {
        self.target_state = Some(state);
        self
    }

    /// Set the event that triggers this transition.
    pub fn on_event(mut self, event: E) -> Self {
        self.event = Some(event);
        self
    }

    /// Add a guard condition to this transition.
    pub fn guard<F>(mut self, guard_fn: F) -> Self
    where
        F: Fn(&S, &E) -> Result<(), String> + 'static,
    {
        self.guards.push(Box::new(guard_fn));
        self
    }

    /// Add an action to be performed during this transition.
    pub fn action<F>(mut self, action_fn: F) -> Self
    where
        F: FnMut(&S, &E) + 'static,
    {
        self.actions.push(Box::new(action_fn));
        self
    }

    /// Build the transition and return a boxed Transition trait object.
    pub fn build(self) -> impl Transition<S, Event = E, Error = TransitionError> + 'static
    where
        S: 'static,
        E: 'static,
    {
        let source_states = self.source_states;
        let target_state = self.target_state.expect("Target state must be set");
        let _event_template = self.event.expect("Event must be set");
        let guards = self.guards;

        // We're going to use Rc<RefCell<...>> to allow mutation inside a Fn closure
        let actions = Rc::new(RefCell::new(self.actions));

        // We'll construct a TypedTransition with a proper Fn implementation
        TypedTransition::<S, E, (), (), _>::new(move |state: &S, event: E| {
            // Check if the current state is a valid source state
            if !source_states.is_empty() && !source_states.iter().any(|s| s == state) {
                return Err(TransitionError::InvalidTransition);
            }

            // Check if all guards pass
            for guard in &guards {
                if let Err(msg) = guard(state, &event) {
                    return Err(TransitionError::GuardFailed(msg));
                }
            }

            // Execute all actions
            // Note: We're using RefCell to allow mutation inside the Fn closure
            if let Ok(mut actions_ref) = actions.try_borrow_mut() {
                for action in &mut *actions_ref {
                    action(state, &event);
                }
            }

            // Return the new state
            Ok(target_state.clone())
        })
    }
}

// Example: Document Workflow System
/// Document states in a workflow system
#[derive(Debug, Clone, PartialEq)]
enum DocumentState {
    Draft,
    Review,
    Approved,
    Published,
    Rejected,
}

/// Events that can trigger state transitions
#[derive(Debug, Clone, PartialEq)]
enum DocumentEvent {
    Submit,
    Approve,
    Reject,
    Publish,
    Revise,
}

/// Document with metadata and content
#[derive(Debug)]
struct Document {
    id: String,
    title: String,
    content: String,
    state: DocumentState,
    author: String,
    reviewer: Option<String>,
}

impl Document {
    fn new(id: &str, title: &str, author: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            content: String::new(),
            state: DocumentState::Draft,
            author: author.to_string(),
            reviewer: None,
        }
    }

    fn set_reviewer(&mut self, reviewer: &str) {
        self.reviewer = Some(reviewer.to_string());
    }

    fn update_content(&mut self, content: &str) {
        self.content = content.to_string();
    }
}

/// The document workflow manager
struct DocumentWorkflow {
    system: TransitionSystem<DocumentState, DocumentEvent>,
    document: Document,
}

impl DocumentWorkflow {
    fn new(document: Document) -> Self {
        let mut system = TransitionSystem::new(document.state.clone());

        // Define transitions

        // Draft -> Review
        let submit = TransitionBuilder::new()
            .from(DocumentState::Draft)
            .to(DocumentState::Review)
            .on_event(DocumentEvent::Submit)
            .guard(|_, _| {
                // In a real system, we might check document length, etc.
                Ok(())
            })
            .build();

        // Review -> Approved
        let approve = TransitionBuilder::new()
            .from(DocumentState::Review)
            .to(DocumentState::Approved)
            .on_event(DocumentEvent::Approve)
            .guard(|_, _| {
                // In a real system, we might verify reviewer permissions
                Ok(())
            })
            .build();

        // Review -> Rejected
        let reject = TransitionBuilder::new()
            .from(DocumentState::Review)
            .to(DocumentState::Rejected)
            .on_event(DocumentEvent::Reject)
            .build();

        // Approved -> Published
        let publish = TransitionBuilder::new()
            .from(DocumentState::Approved)
            .to(DocumentState::Published)
            .on_event(DocumentEvent::Publish)
            .build();

        // Rejected -> Draft
        let revise = TransitionBuilder::new()
            .from(DocumentState::Rejected)
            .to(DocumentState::Draft)
            .on_event(DocumentEvent::Revise)
            .build();

        // Register all transitions
        system.register_transition(submit);
        system.register_transition(approve);
        system.register_transition(reject);
        system.register_transition(publish);
        system.register_transition(revise);

        Self { system, document }
    }

    fn apply_event(&mut self, event: DocumentEvent) -> Result<(), TransitionError> {
        let new_state = self.system.apply_event(event)?;
        self.document.state = new_state.clone();
        println!("Document '{}' transitioned to state: {:?}", self.document.title, new_state);
        Ok(())
    }

    fn current_state(&self) -> &DocumentState {
        self.system.current_state()
    }

    fn possible_transitions(&self) -> Vec<DocumentEvent> {
        let all_events = vec![
            DocumentEvent::Submit,
            DocumentEvent::Approve,
            DocumentEvent::Reject,
            DocumentEvent::Publish,
            DocumentEvent::Revise,
        ];

        self.system.possible_transitions(&all_events)
    }
}

// Tests for the transition system
#[cfg(test)]
mod tests {
    use super::*;

    // Simple traffic light state machine for testing
    #[derive(Debug, Clone, PartialEq)]
    enum TrafficLight {
        Red,
        Yellow,
        Green,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum TrafficEvent {
        Timer,
        Emergency,
        Reset,
    }

    #[test]
    fn test_basic_transitions() {
        let mut system = TransitionSystem::new(TrafficLight::Red);

        // Red -> Green
        let red_to_green = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .to(TrafficLight::Green)
            .on_event(TrafficEvent::Timer)
            .build();

        // Green -> Yellow
        let green_to_yellow = TransitionBuilder::new()
            .from(TrafficLight::Green)
            .to(TrafficLight::Yellow)
            .on_event(TrafficEvent::Timer)
            .build();

        // Yellow -> Red
        let yellow_to_red = TransitionBuilder::new()
            .from(TrafficLight::Yellow)
            .to(TrafficLight::Red)
            .on_event(TrafficEvent::Timer)
            .build();

        // Any -> Red (emergency)
        let emergency = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .from(TrafficLight::Yellow)
            .from(TrafficLight::Green)
            .to(TrafficLight::Red)
            .on_event(TrafficEvent::Emergency)
            .build();

        system.register_transition(red_to_green);
        system.register_transition(green_to_yellow);
        system.register_transition(yellow_to_red);
        system.register_transition(emergency);

        // Test normal cycle
        assert_eq!(*system.current_state(), TrafficLight::Red);
        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Green);
        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Yellow);
        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Red);

        // Test emergency override
        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Green);
        system.apply_event(TrafficEvent::Emergency).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Red);
    }

    #[test]
    fn test_invalid_transition() {
        let mut system = TransitionSystem::new(TrafficLight::Red);

        let red_to_green = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .to(TrafficLight::Green)
            .on_event(TrafficEvent::Timer)
            .build();

        system.register_transition(red_to_green);

        // This should work
        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Green);

        // This should fail (no transition defined from Green with Reset)
        let result = system.apply_event(TrafficEvent::Reset);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TransitionError::InvalidTransition));
    }

    #[test]
    fn test_guard_conditions() {
        let mut system = TransitionSystem::new(TrafficLight::Red);

        // Red -> Green, but only if a guard is satisfied
        let red_to_green = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .to(TrafficLight::Green)
            .on_event(TrafficEvent::Timer)
            .guard(|_, event| {
                if let TrafficEvent::Timer = event {
                    Ok(())
                } else {
                    Err("Not a timer event".to_string())
                }
            })
            .build();

        system.register_transition(red_to_green);

        // This should pass the guard
        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*system.current_state(), TrafficLight::Green);
    }

    #[test]
    fn test_actions() {
        // Use a shared counter to test if the action was called
        let counter = Rc::new(RefCell::new(0));
        let counter_clone = counter.clone();

        let mut system = TransitionSystem::new(TrafficLight::Red);

        // Red -> Green with an action
        let red_to_green = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .to(TrafficLight::Green)
            .on_event(TrafficEvent::Timer)
            .action(move |_, _| {
                *counter_clone.borrow_mut() += 1;
            })
            .build();

        system.register_transition(red_to_green);

        system.apply_event(TrafficEvent::Timer).unwrap();
        assert_eq!(*counter.borrow(), 1);
    }

    #[test]
    fn test_possible_transitions() {
        let mut system = TransitionSystem::new(TrafficLight::Red);

        // Red -> Green
        let red_to_green = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .to(TrafficLight::Green)
            .on_event(TrafficEvent::Timer)
            .build();

        // Red -> Red (emergency)
        let red_emergency = TransitionBuilder::new()
            .from(TrafficLight::Red)
            .to(TrafficLight::Red)
            .on_event(TrafficEvent::Emergency)
            .build();

        system.register_transition(red_to_green);
        system.register_transition(red_emergency);

        let all_events = vec![TrafficEvent::Timer, TrafficEvent::Emergency, TrafficEvent::Reset];

        let possible = system.possible_transitions(&all_events);
        assert_eq!(possible.len(), 2);
        assert!(possible.contains(&TrafficEvent::Timer));
        assert!(possible.contains(&TrafficEvent::Emergency));
        assert!(!possible.contains(&TrafficEvent::Reset));
    }
}

// Main function to demonstrate the document workflow
fn main() {
    let doc = Document::new("DOC-001", "Quarterly Report", "Alice");
    let mut workflow = DocumentWorkflow::new(doc);

    println!("Initial state: {:?}", workflow.current_state());
    println!("Possible transitions: {:?}", workflow.possible_transitions());

    // Walk through the workflow
    println!("\nSubmitting document for review...");
    workflow.apply_event(DocumentEvent::Submit).expect("Failed to submit");
    println!("Possible transitions: {:?}", workflow.possible_transitions());

    println!("\nApproving document...");
    workflow.apply_event(DocumentEvent::Approve).expect("Failed to approve");
    println!("Possible transitions: {:?}", workflow.possible_transitions());

    println!("\nPublishing document...");
    workflow.apply_event(DocumentEvent::Publish).expect("Failed to publish");
    println!("Possible transitions: {:?}", workflow.possible_transitions());

    // This should fail - can't revise a published document
    println!("\nAttempting to revise a published document...");
    match workflow.apply_event(DocumentEvent::Revise) {
        Ok(_) => println!("Successfully revised (unexpected)"),
        Err(e) => println!("Failed as expected: {:?}", e),
    }
}
