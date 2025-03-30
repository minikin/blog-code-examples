use crate::LockFreeStack;
use std::sync::Arc;
use std::thread;

/// Diagnostics function for verifying that the LockFreeStack works correctly.
/// This can be run with `cargo run -- --diagnose`.
pub fn diagnose_lockfree_stack() {
    println!("Starting LockFreeStack diagnostics");

    // Basic push and pop test
    let stack = LockFreeStack::new(true); // Enable verbose mode

    println!("Pushing value 42");
    stack.push(42).expect("Push should succeed");

    println!("Popping value");
    match stack.pop() {
        Some(value) => println!("Popped value: {}", value),
        None => println!("Pop failed - stack was empty"),
    }

    // Concurrent test with hazard pointer protection
    println!("\nTesting concurrent operations with hazard pointer protection");
    let stack = Arc::new(LockFreeStack::new(true));

    // Push a value
    stack.push(42).expect("Push should succeed");

    // Create a clone of the stack for another thread
    let stack_clone = Arc::clone(&stack);

    // Spawn a thread that will pop the value
    let handle = thread::spawn(move || {
        println!("Thread: Popping value from stack");
        let result = stack_clone.pop();
        println!("Thread: Pop result: {:?}", result);
    });

    // Wait for the thread to complete
    handle.join().expect("Thread panicked");

    println!("Diagnostics complete");
}
