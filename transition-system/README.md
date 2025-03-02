# Library Book State Machine

A state machine implementation for tracking the status of library books using Rust.

- [Library Book State Machine](#library-book-state-machine)
  - [Overview](#overview)
  - [Key Features](#key-features)
  - [Project Architecture](#project-architecture)
  - [Running the Example](#running-the-example)
  - [Visualizing the State Machine](#visualizing-the-state-machine)
  - [Visualization Features](#visualization-features)
    - [Text-Based Visualization](#text-based-visualization)
    - [Graphical Visualization](#graphical-visualization)
    - [Example Visualization Output](#example-visualization-output)
  - [Testing](#testing)
  - [Future Improvements](#future-improvements)

## Overview

This project implements a state machine that models the lifecycle of a library book. 
Books can transition between various states such as:

- Available
- Reserved
- Checked Out
- In Transit
- Under Repair
- Lost

Each transition is triggered by events like reservations, check-outs, returns, etc.

## Key Features

- **State Transitions**: Book states change based on defined events
- **Transition History**: Complete history of state changes is recorded
- **Timing Constraints**: State timeouts (e.g., reservations expire after 3 days)
- **Observer Pattern**: Notification system for state changes
- **Persistence**: Save and load state machine status to/from JSON files
- **Visualization Tools**: Generate visual representations of the state machine

## Project Architecture

The codebase has been organized into the following modules:

- `book_state.rs`: Defines the possible states of a book
- `events.rs`: Defines the events that can trigger state transitions
- `system.rs`: Core state machine implementation
- `observers.rs`: Observer pattern implementation for notifications
- `persistence.rs`: Logic for serializing and deserializing the system state
- `visualization.rs`: Tools for visualizing the state machine structure and history

## Running the Example

The main example simulates a book being reserved, checked out, and returned.

```bash
cargo run
```

This will generate two DOT files:
- `initial_state_machine.dot`: A visualization of the state machine structure
- `state_machine_with_path.dot`: A visualization with the transition path highlighted

## Visualizing the State Machine

The project generates DOT format files that can be rendered with Graphviz. To render these files as images:

1. Install Graphviz: `brew install graphviz` (macOS) or follow [instructions for your OS](https://graphviz.org/download/)
2. Generate a PNG from the DOT file:
   ```
   dot -Tpng initial_state_machine.dot -o state_machine.png
   ```

You can also use online DOT renderers like [Graphviz Online](https://dreampuf.github.io/GraphvizOnline/) or [Viz.js](http://viz-js.com/).

## Visualization Features

The state machine visualization tools provide several ways to understand the structure and behavior:

### Text-Based Visualization

1. **State Machine Structure**: `StateVisualization::print_state_machine(&system)` 
   - Displays all states and their possible transitions
   - Shows timing constraints for each state

2. **Transition History**: `StateVisualization::visualize_history(system.get_history())`
   - Shows the sequence of state transitions with emoji for better readability
   - Displays the events that triggered each transition

3. **State Statistics**: `StateVisualization::print_stats(&system)`
   - Shows the number of states, transitions, and history entries
   - Provides statistics on which states were visited and how many times

### Graphical Visualization

1. **DOT Graph Generation**: `StateVisualization::generate_dot(&system, highlight_path)`
   - Creates DOT format files for rendering with Graphviz
   - Option to highlight the actual path taken through the state machine

2. **Markdown Table**: `StateVisualization::history_table(system.get_history())`
   - Generates a markdown-formatted table of transitions
   - Useful for documentation or reports

### Example Visualization Output

When you run the example, the console will show:
- The complete state machine structure with all possible transitions
- A visualization of the path taken through the state machine
- Statistics about state transitions

Additionally, two DOT files are generated:
- `initial_state_machine.dot`: Shows the complete state machine structure
- `state_machine_with_path.dot`: Highlights the actual path taken during execution

These DOT files can be rendered to images using Graphviz or online DOT renderers.

## Testing

The code includes unit tests that verify state transitions, history tracking, and timing constraints:

```bash
cargo test
```

## Future Improvements

1. **Generic State Machine**:
   - Make the state machine implementation generic to work with any state/event types
   - Allow for more complex state machine configurations

2. **Async Support**:
   - Add async versions of the state machine methods for non-blocking operations

3. **Configuration**:
   - Allow external configuration of the state machine through config files

4. **Performance**:
   - Optimize transition lookups for large state machines 