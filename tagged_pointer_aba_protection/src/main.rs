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
