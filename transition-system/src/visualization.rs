use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Write,
    path::Path,
};

use crate::{
    book_state::BookState,
    events::BookEvent,
    system::{LibrarySystem, StateTransition},
};

/// Visualization tools for state machines
#[derive(Debug)]
pub struct StateVisualization;

impl StateVisualization {
    /// Generate a textual representation of the state machine
    pub fn print_state_machine(system: &LibrarySystem) {
        println!("=== State Machine Structure ===");
        println!("Current state: {:?}", system.current_state());

        // Get all transitions from the system
        let transitions = system.get_all_transitions();

        // Group transitions by source state for better readability
        let mut transitions_by_source: HashMap<usize, Vec<(BookEvent, usize)>> = HashMap::new();

        for ((from, event), to) in transitions {
            transitions_by_source.entry(*from).or_default().push((event.clone(), *to));
        }

        // Print all states and their transitions
        for (state_idx, state) in system.get_states().iter().enumerate() {
            println!("\nState {state_idx}: {state:?}");

            if let Some(transitions) = transitions_by_source.get(&state_idx) {
                for (event, to_state_idx) in transitions {
                    println!(
                        "  --({event:?})--> State {to_state_idx}: {:?}",
                        system.get_states().get(*to_state_idx).unwrap_or(&BookState::Available)
                    );
                }
            } else {
                println!("  (No outgoing transitions)");
            }
        }

        println!("\n=== Timing Constraints ===");
        for (state_idx, constraint) in system.get_timing_constraints() {
            println!(
                "State {}: {:?} - Timeout after {:?} seconds, triggers {:?}",
                state_idx,
                system.get_states().get(*state_idx).unwrap_or(&BookState::Available),
                constraint.max_duration.as_secs(),
                constraint.timeout_event
            );
        }
    }

    /// Generate a DOT graph representation of the state machine
    #[must_use]
    pub fn generate_dot(system: &LibrarySystem, highlight_path: bool) -> String {
        let mut dot = String::from("digraph state_machine {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=circle, style=filled, fillcolor=lightblue];\n");

        // Add states
        for (idx, state) in system.get_states().iter().enumerate() {
            // Format the state label, properly escaping quotes
            let state_label = match state {
                BookState::Available => "Available".to_string(),
                BookState::Reserved(person) => format!("Reserved({person})"),
                BookState::CheckedOut(person) => format!("CheckedOut({person})"),
                BookState::InTransit => "InTransit".to_string(),
                BookState::UnderRepair => "UnderRepair".to_string(),
                BookState::Lost => "Lost".to_string(),
            };

            // Current state is highlighted
            if idx == system.get_current_state_idx() {
                dot.push_str(&format!(
                    "  s{idx} [label=\"{state_label}\", fillcolor=palegreen, peripheries=2];\n",
                ));
            } else {
                dot.push_str(&format!("  s{idx} [label=\"{state_label}\"];\n"));
            }
        }

        // Add transitions
        let transitions = system.get_all_transitions();

        // If highlighting, determine which transitions to highlight
        let mut highlighted_transitions = HashSet::new();
        if highlight_path && !system.get_history().is_empty() {
            // Get transitions from history
            #[allow(clippy::arithmetic_side_effects)]
            for i in 0..system.get_history().len() - 1 {
                if let Some(current) = system.get_history().get(i) {
                    // Find the state indices
                    let from_idx = system.get_state_idx(&current.from);
                    let to_idx = system.get_state_idx(&current.to);

                    if let (Some(from), Some(to)) = (from_idx, to_idx) {
                        highlighted_transitions.insert((from, to));
                    }
                }
            }
        }

        // Add all transitions to the graph
        for ((from, event), to) in transitions {
            let style = if highlight_path && highlighted_transitions.contains(&(*from, *to)) {
                "color=red, penwidth=2.0"
            } else {
                "color=black"
            };

            // Format the event label, escaping quotes
            #[allow(clippy::single_char_pattern)]
            let event_label = format!("{event:?}").replace("\"", "\\\"");

            dot.push_str(&format!("  s{from} -> s{to} [label=\"{event_label}\", {style}];\n"));
        }

        dot.push_str("}\n");
        dot
    }

    /// Save the DOT representation to a file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or written to
    pub fn save_dot_to_file(dot: &str, filename: &str) -> Result<(), std::io::Error> {
        let path = Path::new(filename);
        let mut file = File::create(path)?;
        file.write_all(dot.as_bytes())?;
        Ok(())
    }

    /// Generate a visualization of the state machine history
    #[allow(clippy::arithmetic_side_effects)]
    pub fn visualize_history(transitions: &[StateTransition]) {
        println!("=== State Transition History ===");

        if transitions.is_empty() {
            println!("No transitions recorded yet.");
            return;
        }

        for (i, transition) in transitions.iter().enumerate() {
            println!(
                "{}: {} --({:?})--> {}",
                i + 1,
                Self::format_state(&transition.from),
                transition.event,
                Self::format_state(&transition.to)
            );
        }
    }

    /// Format a state for display
    fn format_state(state: &BookState) -> String {
        match state {
            BookState::Available => "📚 Available".to_string(),
            BookState::Reserved(person) => format!("🔖 Reserved({person})"),
            BookState::CheckedOut(person) => format!("📖 CheckedOut({person})"),
            BookState::InTransit => "🚚 InTransit".to_string(),
            BookState::UnderRepair => "🔧 UnderRepair".to_string(),
            BookState::Lost => "❓ Lost".to_string(),
        }
    }

    /// Generate a markdown table of the history
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn history_table(transitions: &[StateTransition]) -> String {
        if transitions.is_empty() {
            return "No transitions recorded yet.".to_string();
        }

        let mut table = String::from("| # | From | Event | To |\n");
        table.push_str("|---|------|-------|----|\n");

        for (i, transition) in transitions.iter().enumerate() {
            table.push_str(&format!(
                "| {} | {} | {:?} | {} |\n",
                i + 1,
                Self::format_state(&transition.from),
                transition.event,
                Self::format_state(&transition.to)
            ));
        }

        table
    }

    /// Print a summary of available state machine statistics
    #[allow(clippy::arithmetic_side_effects)]
    pub fn print_stats(system: &LibrarySystem) {
        println!("=== State Machine Statistics ===");
        println!("Total states: {}", system.get_states().len());
        println!("Total transitions defined: {}", system.get_all_transitions().len());
        println!("Current state: {:?}", system.current_state());
        println!("History entries: {}", system.get_history().len());

        // Count how many times each state was visited
        let mut state_visits = HashMap::new();
        for transition in system.get_history() {
            *state_visits.entry(transition.to.clone()).or_insert(0) += 1;
        }

        println!("\nState visit counts:");
        for (state, count) in state_visits {
            println!("  {state:?}: {count} times");
        }
    }
}
