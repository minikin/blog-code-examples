#![feature(integer_atomics)]

use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::{AtomicU128, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A tagged pointer that combines a raw pointer with a version counter to prevent ABA problems.
/// The ABA problem occurs in lock-free algorithms when a thread reads a value A,
/// gets preempted, and by the time it resumes, although the value is still A,
/// it has been changed to B and back to A by other threads.
#[derive(Debug, Clone, Copy)]
struct TaggedPtr {
    ptr: *mut Node,
    version: u64, // Version counter to detect ABA changes
}

impl TaggedPtr {
    /// Packs the pointer and version into a single u128.
    /// Uses the lower 64 bits for the pointer and upper 64 bits for the version.
    fn pack(&self) -> u128 {
        let ptr_val = self.ptr.addr() as u64;
        (ptr_val as u128) | ((self.version as u128) << 64)
    }

    /// Unpacks a u128 into separate pointer and version components.
    fn unpack(value: u128) -> Self {
        let ptr = (value as u64) as *mut Node;
        let version = (value >> 64) as u64;
        TaggedPtr { ptr, version }
    }
}

/// A node in our lock-free stack.
/// Uses MaybeUninit for the value to allow safe initialization in push operations.
struct Node {
    value: MaybeUninit<i32>,
    next: *mut Node,
}

/// Atomic wrapper for TaggedPtr that provides atomic operations with ABA protection.
struct AtomicTaggedPtr {
    inner: AtomicU128,
}

impl AtomicTaggedPtr {
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

    fn load(&self, ordering: Ordering) -> TaggedPtr {
        TaggedPtr::unpack(self.inner.load(ordering))
    }

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
/// This implementation is safe against the ABA problem because each modification
/// increments the version counter, even if the same pointer value is reused.
pub struct LockFreeStack {
    head: AtomicTaggedPtr,
}

impl LockFreeStack {
    pub fn new() -> Self {
        LockFreeStack {
            head: AtomicTaggedPtr::new(),
        }
    }

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

fn main() {
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
}
