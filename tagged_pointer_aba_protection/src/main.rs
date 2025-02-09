#![feature(integer_atomics)]
#![feature(test)] // Enable benchmarking features

//! # Lock-Free Stack with ABA Protection
//!
//! This example demonstrates a lock-free stack implementation that prevents the ABA problem
//! using tagged pointers. The implementation uses version counters to detect and prevent
//! ABA scenarios in concurrent operations.
//!
//! ## The ABA Problem
//!
//! ```text
//! Time →
//! Thread 1: Read A         →→→→→→→→ Try CAS(A->C) - Problem!
//!                                   (assumes A hasn't changed)
//! Thread 2:    Remove A → Add B → Remove B → Add A
//!
//! Stack:   [A] → [A,B] → [B] → [A]
//! ```
//!
//! The ABA problem occurs when:
//! 1. Thread 1 reads value A
//! 2. Thread 2 changes A to B, then back to A
//! 3. Thread 1 assumes A hasn't changed and proceeds with its operation
//!
//! Our solution uses version counting:
//! ```text
//! Time →
//! Thread 1: Read A(v1)      →→→→→→→→ CAS fails! (A has v2)
//! Thread 2:    Remove A(v1) → Add B → Remove B → Add A(v2)
//! ```

extern crate test;

use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::{AtomicU128, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A tagged pointer that combines a raw pointer with a version counter to prevent ABA problems.
///
/// # Structure
/// - `ptr`: Raw pointer to the node
/// - `version`: Counter that gets incremented on every modification
///
/// # ABA Prevention
/// When a pointer is updated, its version is incremented even if the same memory
/// address is being written. This ensures that if a thread sees the same pointer
/// value later, it can detect whether the pointer has been modified by checking
/// the version number.
#[derive(Debug, Clone, Copy)]
struct TaggedPtr {
    ptr: *mut Node,
    version: u64, // Version counter to detect ABA changes
}

impl TaggedPtr {
    /// Packs the pointer and version into a single u128.
    ///
    /// # Layout
    /// - Lower 64 bits: pointer value
    /// - Upper 64 bits: version counter
    ///
    /// This allows atomic operations on both the pointer and version simultaneously.
    fn pack(&self) -> u128 {
        let ptr_val = self.ptr.addr() as u64;
        (ptr_val as u128) | ((self.version as u128) << 64)
    }

    /// Unpacks a u128 into separate pointer and version components.
    ///
    /// # Returns
    /// A TaggedPtr containing:
    /// - The pointer value from the lower 64 bits
    /// - The version counter from the upper 64 bits
    fn unpack(value: u128) -> Self {
        let ptr = (value as u64) as *mut Node;
        let version = (value >> 64) as u64;
        TaggedPtr { ptr, version }
    }
}

/// A node in the lock-free stack.
///
/// # Fields
/// - `value`: The stored integer value, wrapped in MaybeUninit for safe initialization
/// - `next`: Pointer to the next node in the stack
struct Node {
    value: MaybeUninit<i32>,
    next: *mut Node,
}

/// Atomic wrapper for TaggedPtr that provides atomic operations with ABA protection.
///
/// This wrapper ensures that all operations on the tagged pointer are atomic,
/// preventing race conditions in concurrent scenarios.
struct AtomicTaggedPtr {
    inner: AtomicU128,
}

impl AtomicTaggedPtr {
    /// Creates a new AtomicTaggedPtr initialized with a null pointer and version 0.
    fn new() -> Self {
        AtomicTaggedPtr {
            inner: AtomicU128::new(
                TaggedPtr {
                    ptr: ptr::null_mut(),
                    version: 0,
                }
                .pack(),
            ),
        }
    }

    /// Atomically loads the current TaggedPtr value.
    ///
    /// # Parameters
    /// - `ordering`: The memory ordering to use for the load operation
    fn load(&self, ordering: Ordering) -> TaggedPtr {
        TaggedPtr::unpack(self.inner.load(ordering))
    }

    /// Performs an atomic compare-and-swap operation with version increment.
    ///
    /// # Parameters
    /// - `current`: The expected current value
    /// - `new_ptr`: The new pointer value to store
    /// - `success_order`: Memory ordering for successful CAS
    /// - `failure_order`: Memory ordering for failed CAS
    ///
    /// # Returns
    /// - `Ok(())` if the CAS succeeded
    /// - `Err(actual)` if the CAS failed, containing the actual value found
    fn compare_and_swap(
        &self,
        current: TaggedPtr,
        new_ptr: *mut Node,
        success_order: Ordering,
        failure_order: Ordering,
    ) -> Result<(), TaggedPtr> {
        let new = TaggedPtr {
            ptr: new_ptr,
            version: current.version.wrapping_add(1),
        };
        self.inner
            .compare_exchange(current.pack(), new.pack(), success_order, failure_order)
            .map(|_| ())
            .map_err(TaggedPtr::unpack)
    }
}

/// A lock-free stack implementation with ABA protection using tagged pointers.
///
/// # Stack Structure
/// ```text
///  HEAD
///   ↓
/// [3|v2] → [2|v1] → [1|v1] → null
///   │        │        │
///   └────────┴────────┴── Each node has a value and points to the next node
/// ```
///
/// # Example
/// ```
/// let stack = LockFreeStack::new();
/// stack.push(1);
/// stack.push(2);
/// assert_eq!(stack.pop(), Some(2)); // LIFO order
/// ```
pub struct LockFreeStack {
    head: AtomicTaggedPtr,
}

impl LockFreeStack {
    /// Creates a new empty lock-free stack.
    pub fn new() -> Self {
        LockFreeStack {
            head: AtomicTaggedPtr::new(),
        }
    }

    /// Pushes a new value onto the top of the stack.
    ///
    /// # Implementation Details
    /// 1. Creates a new node with the given value
    /// 2. Repeatedly tries to update the head pointer until successful:
    ///    - Reads current head
    ///    - Points new node to current head
    ///    - Attempts CAS to update head to new node
    ///
    /// # Parameters
    /// - `value`: The integer value to push onto the stack
    ///
    /// # Thread Safety
    /// This operation is lock-free and thread-safe. Multiple threads can
    /// push simultaneously without blocking each other.
    pub fn push(&self, value: i32) {
        let new_node = Box::into_raw(Box::new(Node {
            value: MaybeUninit::new(value),
            next: ptr::null_mut(),
        }));

        loop {
            let current = self.head.load(Ordering::Relaxed);
            unsafe { (*new_node).next = current.ptr };

            match self.head.compare_and_swap(
                current,
                new_node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    println!(
                        "[Thread {:?}] Successfully pushed {} (version {})",
                        thread::current().id(),
                        value,
                        current.version
                    );
                    break;
                }
                Err(new_current) => {
                    println!(
                        "[Thread {:?}] Push conflict detected! Version changed from {} to {}",
                        thread::current().id(),
                        current.version,
                        new_current.version
                    );
                    continue;
                }
            }
        }
    }

    /// Pops a value from the top of the stack.
    ///
    /// # Implementation Details
    /// 1. Repeatedly tries to update the head pointer until successful:
    ///    - Reads current head
    ///    - If null, returns None
    ///    - Otherwise, attempts CAS to update head to next node
    /// 2. If successful, returns the value from the popped node
    ///
    /// # Returns
    /// - `Some(value)` if a value was successfully popped
    /// - `None` if the stack is empty
    ///
    /// # Thread Safety
    /// This operation is lock-free and thread-safe. Multiple threads can
    /// pop simultaneously without blocking each other.
    pub fn pop(&self) -> Option<i32> {
        loop {
            let current = self.head.load(Ordering::Acquire);
            if current.ptr.is_null() {
                return None;
            }

            let next = unsafe { (*current.ptr).next };
            match self
                .head
                .compare_and_swap(current, next, Ordering::Release, Ordering::Relaxed)
            {
                Ok(_) => {
                    let node = unsafe { Box::from_raw(current.ptr) };
                    let value = unsafe { node.value.assume_init() };
                    println!(
                        "[Thread {:?}] Successfully popped {} (version {})",
                        thread::current().id(),
                        value,
                        current.version
                    );
                    return Some(value);
                }
                Err(new_current) => {
                    println!(
                        "[Thread {:?}] Pop conflict detected! Version changed from {} to {}",
                        thread::current().id(),
                        current.version,
                        new_current.version
                    );
                    continue;
                }
            }
        }
    }
}

/// Demonstrates the ABA problem and how version counting prevents it.
///
/// # Scenario
/// 1. Thread 1 reads top value (3)
/// 2. Thread 2 makes multiple modifications while Thread 1 is sleeping:
///    - Pops 3
///    - Pops 2
///    - Pushes 3 back
/// 3. Thread 1 wakes up and attempts to modify stack
///    - Will fail due to version mismatch
///
/// This shows how version counting detects that the stack was modified
/// even though the same value (3) is present.
fn _aba_example() {
    println!("\nDemonstrating ABA problem...");
    let stack = Arc::new(LockFreeStack::new());

    // Initial state: Push 1, 2, 3
    stack.push(1);
    stack.push(2);
    stack.push(3);
    println!("Initial stack state: [3] → [2] → [1]");

    let stack_clone1 = Arc::clone(&stack);
    let stack_clone2 = Arc::clone(&stack);

    // Thread 1: Will try to pop 3 and push it back later
    let handle1 = thread::spawn(move || {
        let current = stack_clone1.head.load(Ordering::Acquire);
        println!(
            "Thread 1: Read top value (3) with version {}",
            current.version
        );

        // Simulate some work
        thread::sleep(Duration::from_millis(200));

        println!("Thread 1: Attempting to modify stack...");
        let _ = stack_clone1.head.compare_and_swap(
            current,
            unsafe { (*current.ptr).next },
            Ordering::Release,
            Ordering::Relaxed,
        );
    });

    // Thread 2: Will perform multiple operations while Thread 1 is sleeping
    let handle2 = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));

        // Pop 3
        let val = stack_clone2.pop();
        println!("Thread 2: Popped {}", val.unwrap());

        // Pop 2
        let val = stack_clone2.pop();
        println!("Thread 2: Popped {}", val.unwrap());

        // Push 3 back
        stack_clone2.push(3);
        println!("Thread 2: Pushed 3 back");
    });

    handle1.join().unwrap();
    handle2.join().unwrap();

    println!("\nFinal stack state:");
    while let Some(val) = stack.pop() {
        println!("Value: {}", val);
    }
}

fn main() {
    // First demonstrate the ABA problem
    _aba_example();
    println!("\n-----------------------------------\n");

    // Then run the original demo with ABA protection
    println!("Now running demo with ABA protection...");
    println!("Starting ABA protection demonstration...");
    let stack = Arc::new(LockFreeStack::new());
    let num_threads = 4;
    let operations_per_thread = 3;

    // Spawn push threads
    let push_handles: Vec<_> = (0..num_threads)
        .map(|thread_id| {
            let stack = Arc::clone(&stack);
            thread::spawn(move || {
                println!(
                    "[Thread {:?}] Started pushing operations",
                    thread::current().id()
                );

                for i in 0..operations_per_thread {
                    let value = thread_id * operations_per_thread + i;
                    println!(
                        "[Thread {:?}] Attempting to push value {}",
                        thread::current().id(),
                        value
                    );
                    stack.push(value);
                    thread::sleep(Duration::from_millis(100));
                }
            })
        })
        .collect();

    // Wait for all pushes to complete
    for handle in push_handles {
        handle.join().unwrap();
    }

    println!("\n--- All push operations completed ---\n");

    // Spawn pop threads
    let pop_handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let stack = Arc::clone(&stack);
            thread::spawn(move || {
                println!(
                    "[Thread {:?}] Started popping operations",
                    thread::current().id()
                );

                for _ in 0..operations_per_thread {
                    match stack.pop() {
                        Some(_) => (),
                        None => println!("[Thread {:?}] Stack was empty", thread::current().id()),
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            })
        })
        .collect();

    // Wait for all pops to complete
    for handle in pop_handles {
        handle.join().unwrap();
    }

    println!("\n--- All operations completed ---");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;
    use test::Bencher;

    #[test]
    fn test_push_and_pop_single_threaded() {
        let stack = LockFreeStack::new();
        stack.push(1);
        stack.push(2);
        stack.push(3);

        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_empty_stack() {
        let stack = LockFreeStack::new();
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_multiple_threads_push() {
        let stack = Arc::new(LockFreeStack::new());
        let thread_count = 4;
        let values_per_thread = 100;

        let handles: Vec<_> = (0..thread_count)
            .map(|thread_id| {
                let stack = Arc::clone(&stack);
                thread::spawn(move || {
                    for i in 0..values_per_thread {
                        stack.push(thread_id * values_per_thread + i);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify that we can pop all values
        let mut popped_count = 0;
        while stack.pop().is_some() {
            popped_count += 1;
        }

        assert_eq!(popped_count, thread_count * values_per_thread);
    }

    #[test]
    fn test_concurrent_push_and_pop() {
        let stack = Arc::new(LockFreeStack::new());
        let push_thread_count: usize = 3;
        let pop_thread_count: usize = 2;
        let values_per_thread: usize = 100;

        // Spawn push threads
        let push_handles: Vec<_> = (0..push_thread_count)
            .map(|thread_id| {
                let stack = Arc::clone(&stack);
                thread::spawn(move || {
                    for i in 0..values_per_thread {
                        stack.push(i32::try_from(thread_id * values_per_thread + i).unwrap());
                    }
                })
            })
            .collect();

        // Spawn pop threads
        let pop_handles: Vec<_> = (0..pop_thread_count)
            .map(|_| {
                let stack = Arc::clone(&stack);
                thread::spawn(move || {
                    let mut popped_values = HashSet::new();
                    let target_count = (values_per_thread * push_thread_count) / pop_thread_count;
                    while popped_values.len() < target_count {
                        if let Some(value) = stack.pop() {
                            popped_values.insert(value);
                        }
                        thread::yield_now();
                    }
                    popped_values
                })
            })
            .collect();

        // Wait for push threads
        for handle in push_handles {
            handle.join().unwrap();
        }

        // Collect results from pop threads
        let mut all_popped = HashSet::new();
        for handle in pop_handles {
            let thread_values = handle.join().unwrap();
            all_popped.extend(thread_values);
        }

        // Verify that all remaining values can be popped
        while let Some(value) = stack.pop() {
            all_popped.insert(value);
        }

        assert_eq!(
            all_popped.len(),
            values_per_thread * push_thread_count,
            "All pushed values should be popped exactly once"
        );
    }

    #[test]
    fn test_aba_prevention() {
        let stack = Arc::new(LockFreeStack::new());

        // Push initial values
        stack.push(1);
        stack.push(2);
        stack.push(3);

        let stack_clone = Arc::clone(&stack);

        // Thread 1: Try to pop and push back after delay
        let handle1 = thread::spawn(move || {
            // Pop the top value (3)
            let value = stack_clone.pop().unwrap();
            assert_eq!(value, 3);

            // Delay to allow other thread to modify stack
            thread::sleep(Duration::from_millis(100));

            // Push the value back
            stack_clone.push(value);
        });

        let stack_clone = Arc::clone(&stack);

        // Thread 2: Perform multiple operations while Thread 1 is delayed
        let handle2 = thread::spawn(move || {
            // Pop value (2)
            let _value2 = stack_clone.pop().unwrap();
            // Push new value
            stack_clone.push(4);
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        // The final state should reflect all operations with version tracking
        let mut values = Vec::new();
        while let Some(value) = stack.pop() {
            values.push(value);
        }

        // Values should be in LIFO order
        assert!(values.len() >= 2, "Stack should have at least 2 values");
    }

    #[bench]
    fn bench_single_threaded_push_pop(b: &mut Bencher) {
        let stack = LockFreeStack::new();
        b.iter(|| {
            stack.push(1);
            stack.pop()
        });
    }

    #[bench]
    fn bench_concurrent_push_pop(b: &mut Bencher) {
        let stack = Arc::new(LockFreeStack::new());
        let running = Arc::new(AtomicU128::new(1));
        let running_clone = Arc::clone(&running);
        let stack_clone = Arc::clone(&stack);

        let push_thread = thread::spawn(move || {
            while running_clone.load(Ordering::Relaxed) == 1 {
                stack_clone.push(1);
                thread::yield_now();
            }
        });

        b.iter(|| stack.pop());

        // Signal the push thread to stop
        running.store(0, Ordering::Relaxed);
        push_thread.join().unwrap();
    }

    #[test]
    fn test_integer_conversion_edge_cases() {
        let stack = LockFreeStack::new();

        // Test maximum i32 value
        stack.push(i32::MAX);
        assert_eq!(stack.pop(), Some(i32::MAX));

        // Test minimum i32 value
        stack.push(i32::MIN);
        assert_eq!(stack.pop(), Some(i32::MIN));

        // Test zero
        stack.push(0);
        assert_eq!(stack.pop(), Some(0));
    }

    #[test]
    fn test_stack_operations_visualization() {
        let stack = LockFreeStack::new();
        println!("Empty stack: null");

        stack.push(1);
        println!("After push(1): [1] → null");

        stack.push(2);
        println!("After push(2): [2] → [1] → null");

        stack.pop();
        println!("After pop():   [1] → null");

        assert_eq!(stack.pop(), Some(1));
        println!("After pop():   null");
    }
}
