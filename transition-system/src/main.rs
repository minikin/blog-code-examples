//! Transition System Example Application
//!
//! This is an example application demonstrating a state transition system
//! for tracking library books through various states.

use std::time::Duration;

use transition_system::{
    StateVisualization,
    book_state::BookState,
    events::BookEvent,
    observers::{NotificationService, TransitionLogger},
    system::LibrarySystem,
};

/// Set up the library state machine with all states, transitions and timing constraints
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

    // Visualize the initial state machine structure
    println!("\n==== Initial State Machine Visualization ====\n");
    StateVisualization::print_state_machine(&book_system);

    // Generate and save DOT graph of the state machine
    let dot = StateVisualization::generate_dot(&book_system, false);
    match StateVisualization::save_dot_to_file(&dot, "initial_state_machine.dot") {
        Ok(()) => println!("\nState machine graph saved to 'initial_state_machine.dot'"),
        Err(e) => println!("\nFailed to save state machine graph: {e}"),
    }

    // Simulate book lifecycle with transition system
    println!("\n==== Book Lifecycle Simulation ====\n");
    println!("Initial state: {book_system}");

    // Alice reserves the book
    match book_system.process_event(BookEvent::Reserve("Alice".to_string())) {
        Ok(_) => println!("New state: {book_system}"),
        Err(e) => println!("Error: {e}"),
    }

    // Alice checks out the book
    match book_system.process_event(BookEvent::CheckOut("Alice".to_string())) {
        Ok(_) => println!("New state: {book_system}"),
        Err(e) => println!("Error: {e}"),
    }

    // Alice returns the book
    match book_system.process_event(BookEvent::Return) {
        Ok(_) => println!("New state: {book_system}"),
        Err(e) => println!("Error: {e}"),
    }

    // Print the transition history using improved visualization
    println!("\n==== State Transition History Visualization ====\n");
    StateVisualization::visualize_history(book_system.get_history());

    // Generate and save DOT graph with the path highlighted
    let dot_with_path = StateVisualization::generate_dot(&book_system, true);
    match StateVisualization::save_dot_to_file(&dot_with_path, "state_machine_with_path.dot") {
        Ok(()) => {
            println!("\nState machine graph with path saved to 'state_machine_with_path.dot'");
        }
        Err(e) => println!("\nFailed to save state machine graph with path: {e}"),
    }

    // Print statistics about the state machine
    println!("\n==== State Machine Statistics ====\n");
    StateVisualization::print_stats(&book_system);

    // Generate and save a markdown table of the transition history
    println!("\n==== Markdown Table of Transitions ====\n");
    let markdown_table = StateVisualization::history_table(book_system.get_history());
    println!("{markdown_table}");

    // Save the markdown table to a file
    match std::fs::write("transition_history.md", markdown_table) {
        Ok(()) => println!("\nTransition history table saved to 'transition_history.md'"),
        Err(e) => println!("\nFailed to save transition history table: {e}"),
    }

    // Save the state to a file before simulating a restart
    if let Err(e) = book_system.save_state_to_file() {
        println!("Error saving state: {e}");
    }

    // In a real application, we would have a completely separate run here
    // To simulate this, we'll load the state from the file
    println!("\n--- Simulating application restart ---\n");

    match LibrarySystem::load_state_from_file("book-1234") {
        Ok(mut loaded_system) => {
            println!("Successfully loaded system from file: {loaded_system}");

            // Continue working with the loaded system
            match loaded_system.process_event(BookEvent::SendToRepair) {
                Ok(_) => println!("New state after loading: {loaded_system}"),
                Err(e) => println!("Error after loading: {e}"),
            }

            // Print the history (should include previous transitions too)
            loaded_system.print_history();
        }
        Err(e) => {
            println!("Failed to load system: {e}");
        }
    }
}
