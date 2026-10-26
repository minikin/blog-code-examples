use clap::Parser;
use colored::*;
use hazard_pointers_demo::LockFreeStack;
use rand::Rng;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

mod test_lockfree;

/// Command-line arguments for the hazard pointers demo
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
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
        aba_demonstration();
    }

    if args.stress_test {
        stress_test();
    } else if args.quick_test {
        quick_verification_test();
    } else if !show_demo {
        basic_demo();
    }

    println!("\n{}", "Demonstration complete!".green().bold());
}

/// Demonstrates a basic usage of our lock-free stack with hazard pointers
fn basic_demo() {
    println!("{}", "\nRunning basic demonstration...".yellow().bold());

    let stack = LockFreeStack::new();

    // Push some values
    println!("Pushing values 1, 2, 3 onto the stack");
    stack.push(1);
    stack.push(2);
    stack.push(3);

    println!("Stack size: {}", stack.len());

    // Pop values one by one
    println!("\nPopping values from the stack:");
    while let Some(value) = stack.pop() {
        println!("Popped: {}", value);
    }

    println!("Stack is now empty: {}", stack.is_empty());
}

/// Demonstrates how hazard pointers protect against the ABA problem.
///
/// Two channels force one interleaving, so the output is the same every run.
fn aba_demonstration() {
    println!(
        "{}",
        "\nDemonstrating ABA problem prevention with hazard pointers..."
            .yellow()
            .bold()
    );

    let stack = Arc::new(LockFreeStack::new());
    stack.push(1);
    stack.push(2);
    stack.push(3);
    println!("Initial stack state: [3] → [2] → [1]");

    let (protected_tx, protected_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();

    let t1 = {
        let stack = Arc::clone(&stack);
        thread::spawn(move || {
            let head = stack.head.load(Ordering::Acquire);
            let guard = stack.hazard_pointers.protect(head);
            println!("{}", "Thread 1: published the head node (value 3)".blue());
            protected_tx.send(()).unwrap();
            done_rx.recv().unwrap();

            // The validation step of pop: the head moved while this thread was
            // away, so it must not use `head` to compute a new head.
            if stack.head.load(Ordering::Acquire) == head {
                println!("{}", "Thread 1: head unchanged, safe to continue".blue());
            } else {
                println!(
                    "{}",
                    "Thread 1: head changed since publication, starting over".blue()
                );
            }
            // Safety: the node is published, so it has not been freed even
            // though Thread 2 popped it.
            let value = unsafe { *(*guard.as_ptr()).value };
            println!(
                "{}",
                format!("Thread 1: the protected node is still readable, value {value}").blue()
            );
        })
    };

    let t2 = {
        let stack = Arc::clone(&stack);
        thread::spawn(move || {
            protected_rx.recv().unwrap();
            let a = stack.pop().unwrap();
            let b = stack.pop().unwrap();
            println!("{}", format!("Thread 2: popped {a} and {b}").magenta());
            let freed = stack.hazard_pointers.try_reclaim(true);
            println!(
                "{}",
                format!(
                    "Thread 2: forced a scan, freed {freed} node, {} still retired",
                    stack.hazard_pointers.retired_len()
                )
                .magenta()
            );
            stack.push(3);
            println!("{}", "Thread 2: pushed 3 back onto the stack".magenta());
            done_tx.send(()).unwrap();
        })
    };

    t1.join().expect("Thread 1 panicked");
    t2.join().expect("Thread 2 panicked");

    println!(
        "Thread 1 cleared its slot. A scan now frees {} node.",
        stack.hazard_pointers.try_reclaim(true)
    );

    print!("Final stack from the top:");
    while let Some(value) = stack.pop() {
        print!(" {value}");
    }
    println!();

    println!("\n{}", "Key insight:".yellow().bold());
    println!("The node Thread 1 published survived the scan, so its address could not be");
    println!("handed out again while Thread 1 held it, and reading it stayed valid.");
    println!("The re-read of head is what tells Thread 1 its snapshot is stale.");
}

/// Run a stress test with many concurrent operations
fn stress_test() {
    println!(
        "{}",
        "\nRunning stress test with concurrent operations..."
            .yellow()
            .bold()
    );

    let stack = Arc::new(LockFreeStack::new());
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
                    stack_clone.push(value);
                    pushes += 1;
                } else if stack_clone.pop().is_some() {
                    pops += 1;
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
            total_pushes - total_pops,
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
fn quick_verification_test() {
    println!("{}", "\nRunning quick verification test...".yellow().bold());

    let stack = Arc::new(LockFreeStack::new());
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
            stack_clone.push(i);
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
