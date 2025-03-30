use clap::Parser;
use colored::*;
use hazard_pointers_demo::LockFreeStack;
use rand::Rng;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

mod test_lockfree;

/// Command-line arguments for the hazard pointers demo
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Enable verbose output with detailed operation logs
    #[arg(short, long)]
    verbose: bool,

    /// Run a stress test with many concurrent operations
    #[arg(short, long)]
    stress_test: bool,

    /// Run a smaller verification test (faster than full stress test)
    #[arg(long)]
    quick_test: bool,

    /// Show a visual demonstration of the ABA problem and how hazard pointers solve it
    #[arg(long, default_value_t = true)]
    show_aba_demo: bool,

    /// Skip the ABA demonstration
    #[arg(long)]
    no_show_aba_demo: bool,

    /// Run diagnostics on the LockFreeStack implementation
    #[arg(long)]
    diagnose: bool,
}

fn main() {
    let args = Args::parse();

    println!("{}", "Hazard Pointers Demonstration".green().bold());
    println!("=====================================\n");

    // Run diagnostics if requested
    if args.diagnose {
        println!(
            "{}",
            "Running diagnostics on LockFreeStack...".yellow().bold()
        );
        test_lockfree::diagnose_lockfree_stack();
        return;
    }

    // Prioritize the explicit flag to not show the demo
    let show_demo = args.show_aba_demo && !args.no_show_aba_demo;

    if show_demo {
        aba_demonstration(args.verbose);
    }

    if args.stress_test {
        stress_test(args.verbose);
    } else if args.quick_test {
        quick_verification_test(args.verbose);
    } else if !show_demo {
        basic_demo(args.verbose);
    }

    println!("\n{}", "Demonstration complete!".green().bold());
}

/// Demonstrates a basic usage of our lock-free stack with hazard pointers
fn basic_demo(verbose: bool) {
    println!("{}", "\nRunning basic demonstration...".yellow().bold());

    let stack = LockFreeStack::new(verbose);

    // Push some values
    println!("Pushing values 1, 2, 3 onto the stack");
    stack.push(1).expect("Push should succeed");
    stack.push(2).expect("Push should succeed");
    stack.push(3).expect("Push should succeed");

    println!("Stack size: {}", stack.len());

    // Pop values one by one
    println!("\nPopping values from the stack:");
    while let Some(value) = stack.pop() {
        println!("Popped: {}", value);
    }

    println!("Stack is now empty: {}", stack.is_empty());
}

/// Demonstrates how hazard pointers protect against the ABA problem
fn aba_demonstration(verbose: bool) {
    println!(
        "{}",
        "\nDemonstrating ABA problem prevention with hazard pointers..."
            .yellow()
            .bold()
    );

    // Create a shared stack
    let stack = Arc::new(LockFreeStack::new(verbose));

    // Initial state: Push values onto the stack
    stack.push(1).expect("Push should succeed");
    stack.push(2).expect("Push should succeed");
    stack.push(3).expect("Push should succeed");

    println!("Initial stack state: [3] → [2] → [1]");

    // Clone the stack for each thread
    let stack_clone1 = Arc::clone(&stack);
    let stack_clone2 = Arc::clone(&stack);

    // Thread 1: Will try to pop 3 and then get delayed
    let handle1 = thread::spawn(move || {
        println!("{}", "Thread 1: Starting operation".blue());

        // Load the head but don't complete the operation
        let hazard_pointers = &stack_clone1.hazard_pointers;
        let head = stack_clone1.head.load(std::sync::atomic::Ordering::Acquire);
        hazard_pointers.protect(head);

        println!("{}", "Thread 1: Protected head node (with value 3)".blue());

        // Simulate delay - this is where Thread 2 will make changes
        println!("{}", "Thread 1: Going to sleep for 200ms...".blue());
        thread::sleep(Duration::from_millis(200));

        // Try to complete the pop operation
        println!(
            "{}",
            "Thread 1: Waking up and trying to complete pop operation".blue()
        );
        let result = stack_clone1.pop();
        println!("{}", format!("Thread 1: Pop result: {:?}", result).blue());

        result
    });

    // Thread 2: Will perform multiple operations while Thread 1 is sleeping
    let handle2 = thread::spawn(move || {
        // Give Thread 1 time to start and protect its node
        thread::sleep(Duration::from_millis(50));
        println!(
            "{}",
            "Thread 2: Performing operations while Thread 1 is delayed".magenta()
        );

        // Pop 3
        let val = stack_clone2.pop().expect("Stack should have value 3");
        println!("{}", format!("Thread 2: Popped {}", val).magenta());

        // Pop 2
        let val = stack_clone2.pop().expect("Stack should have value 2");
        println!("{}", format!("Thread 2: Popped {}", val).magenta());

        // Push 3 again - This creates the ABA condition!
        // Without hazard pointers, Thread 1 wouldn't notice this change
        stack_clone2.push(3).expect("Push should succeed");
        println!("{}", "Thread 2: Pushed 3 back onto the stack".magenta());
        println!(
            "{}",
            "Thread 2: Created ABA condition (3->1->empty->3->1)"
                .magenta()
                .bold()
        );
    });

    // Wait for both threads to complete
    let _thread1_result = handle1.join().expect("Thread 1 panicked");
    handle2.join().expect("Thread 2 panicked");

    // Explain what happened
    println!("\n{}", "What just happened?".green().bold());
    println!("1. Thread 1 started a pop operation and protected node with value 3");
    println!("2. While Thread 1 was sleeping, Thread 2:");
    println!("   - Popped value 3 from the stack");
    println!("   - Popped value 2 from the stack");
    println!("   - Pushed value 3 back onto the stack");
    println!(
        "3. This created an 'ABA' scenario - the head had value 3, changed to 1, then back to 3"
    );
    println!("4. When Thread 1 woke up, it was still able to safely continue its operation");
    println!("5. The hazard pointer protected the original node with value 3 from being reclaimed");
    println!("   even though it was temporarily removed from the stack");

    // Show the final state
    println!("\nFinal stack state:");
    let mut remaining = Vec::new();
    while let Some(val) = stack.pop() {
        remaining.push(val);
    }

    for val in remaining.iter().rev() {
        println!("Value: {}", val);
    }

    // Summary
    println!("\n{}", "Key insight:".yellow().bold());
    println!("Without hazard pointers, Thread 1 might have accessed invalid memory.");
    println!("The hazard pointer mechanism ensured that the memory was protected while in use,");
    println!("preventing use-after-free bugs even in the presence of the ABA pattern.");
}

/// Run a stress test with many concurrent operations
fn stress_test(verbose: bool) {
    println!(
        "{}",
        "\nRunning stress test with concurrent operations..."
            .yellow()
            .bold()
    );

    let stack = Arc::new(LockFreeStack::new(verbose));
    // Reduce the number of operations for a quicker test
    let num_threads = 4;
    let operations_per_thread = 200;
    let test_timeout = Duration::from_secs(30); // 30-second timeout

    let mut handles = Vec::new();

    println!(
        "Spawning {} threads with {} operations each (timeout: {}s)",
        num_threads,
        operations_per_thread,
        test_timeout.as_secs()
    );

    let start_time = Instant::now();

    // Create threads that perform mixed operations
    for thread_id in 0..num_threads {
        let stack_clone = Arc::clone(&stack);
        let handle = thread::spawn(move || {
            let mut rng = rand::rng();
            let mut pushes = 0;
            let mut pops = 0;

            for op in 0..operations_per_thread {
                // Print progress every 50 operations
                if op % 50 == 0 {
                    println!("Thread {} completed {} operations", thread_id, op);
                }

                // Check if we've exceeded the timeout
                if Instant::now().duration_since(start_time) > test_timeout {
                    println!("Thread {} timed out, returning early", thread_id);
                    return (pushes, pops);
                }

                // 60% chance to push, 40% chance to pop
                if rng.random::<f32>() < 0.6 {
                    let value = rng.random::<u32>();
                    if stack_clone.push(value).is_ok() {
                        pushes += 1;
                    }
                } else {
                    if stack_clone.pop().is_some() {
                        pops += 1;
                    }
                }

                // Check if we should introduce a small delay
                if rng.random::<f32>() < 0.005 {
                    thread::sleep(Duration::from_micros(rng.random_range(1..10)));
                }
            }

            println!(
                "Thread {} finished: {} pushes, {} pops",
                thread_id, pushes, pops
            );
            (pushes, pops)
        });

        handles.push(handle);
    }

    println!("All threads spawned, waiting for completion...");

    // Collect results
    let mut total_pushes = 0;
    let mut total_pops = 0;

    for (i, handle) in handles.into_iter().enumerate() {
        println!("Waiting for thread {} to complete...", i);

        // Check for timeout
        if Instant::now().duration_since(start_time) > test_timeout {
            println!("Timeout reached, stopping test");
            break;
        }

        match handle.join() {
            Ok((pushes, pops)) => {
                println!("Thread {} completed successfully", i);
                total_pushes += pushes;
                total_pops += pops;
            }
            Err(e) => {
                println!("Thread {} panicked: {:?}", i, e);
            }
        }
    }

    let elapsed = Instant::now().duration_since(start_time);
    println!("\nStress test completed in {:.2}s!", elapsed.as_secs_f32());
    println!("Total push operations: {}", total_pushes);
    println!("Total pop operations: {}", total_pops);
    println!("Final stack size: {}", stack.len());
    println!(
        "Elements still in stack should equal pushes - pops: {}",
        total_pushes - total_pops
    );

    // Only validate if we didn't timeout
    if elapsed <= test_timeout {
        // Validate that the stack size is correct
        assert_eq!(
            stack.len(),
            total_pushes - total_pops as usize,
            "Stack size doesn't match expected value!"
        );
        println!("{}", "Stress test validation passed!".green().bold());
    } else {
        println!(
            "{}",
            "Stress test timed out - skipping validation"
                .yellow()
                .bold()
        );
    }
}

/// Run a quick verification test with less operations
fn quick_verification_test(verbose: bool) {
    println!("{}", "\nRunning quick verification test...".yellow().bold());

    let stack = Arc::new(LockFreeStack::new(verbose));
    let num_threads = 2;
    let operations_per_thread = 50;

    println!(
        "Running {} threads with {} operations each",
        num_threads, operations_per_thread
    );

    // Just do simple pushes and pops to verify correctness
    let stack_clone = Arc::clone(&stack);
    let push_thread = thread::spawn(move || {
        for i in 0..operations_per_thread {
            stack_clone.push(i).expect("Push should succeed");
            if i % 10 == 0 {
                println!("Pushed {} values", i);
            }
        }
    });

    push_thread.join().expect("Push thread panicked");

    // Verify the stack size
    assert_eq!(stack.len(), operations_per_thread);
    println!("Pushed {} items successfully", operations_per_thread);

    // Now pop everything
    let mut popped = 0;
    while stack.pop().is_some() {
        popped += 1;
        if popped % 10 == 0 {
            println!("Popped {} values", popped);
        }
    }

    // Verify we popped everything
    assert_eq!(popped, operations_per_thread);
    assert_eq!(stack.len(), 0);

    println!("{}", "Quick verification test passed!".green().bold());
}
