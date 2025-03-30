use std::collections::HashSet;
use std::fmt;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

/// A thread-local hazard pointer registry
///
/// This struct maintains a list of pointers that a thread is currently using,
/// protecting them from being reclaimed by other threads.
pub struct HazardPointers<T> {
    /// Map from thread ID to list of hazard pointers
    thread_hazards: Mutex<Vec<(ThreadId, *mut T)>>,
    /// Global retirement list of nodes awaiting safe reclamation
    retire_list: Mutex<Vec<*mut T>>,
}

// Safety: HazardPointers can be safely shared between threads because
// all its mutations are protected by internal mutexes
unsafe impl<T> Send for HazardPointers<T> {}
unsafe impl<T> Sync for HazardPointers<T> {}

impl<T> HazardPointers<T> {
    /// Creates a new hazard pointer registry
    pub fn new() -> Self {
        HazardPointers {
            thread_hazards: Mutex::new(Vec::new()),
            retire_list: Mutex::new(Vec::new()),
        }
    }

    /// Registers a hazard pointer for the current thread
    ///
    /// This protects the given pointer from being reclaimed by other threads
    /// until explicitly cleared with clear_hazards().
    pub fn protect(&self, ptr: *mut T) -> *mut T {
        if !ptr.is_null() {
            let thread_id = thread::current().id();
            let mut hazards = self
                .thread_hazards
                .lock()
                .expect("Failed to lock hazard list - mutex poisoned");

            // Check if we already have an entry for this thread
            for entry in hazards.iter_mut() {
                if entry.0 == thread_id {
                    entry.1 = ptr;
                    return ptr;
                }
            }

            // No existing entry, add a new one
            hazards.push((thread_id, ptr));
        }
        ptr
    }

    /// Clears all hazard pointers for the current thread
    ///
    /// This should be called when the thread no longer needs to access
    /// previously protected pointers.
    pub fn clear_hazards(&self) {
        let thread_id = thread::current().id();
        let mut hazards = self
            .thread_hazards
            .lock()
            .expect("Failed to lock hazard list - mutex poisoned");
        hazards.retain(|entry| entry.0 != thread_id);
    }

    /// Adds a pointer to the retirement list for later reclamation
    ///
    /// The memory will be reclaimed when it's safe to do so (i.e., when no thread
    /// has it marked as hazardous).
    pub fn retire(&self, ptr: *mut T) {
        if !ptr.is_null() {
            let mut retire = self
                .retire_list
                .lock()
                .expect("Failed to lock retire list - mutex poisoned");
            retire.push(ptr);

            // Attempt to reclaim memory if retire list is getting large
            if retire.len() > 10 {
                self.try_reclaim(false);
            }
        }
    }

    /// Attempts to reclaim memory from the retirement list
    ///
    /// This scans all hazard pointers across all threads and only reclaims
    /// memory that isn't protected by any thread.
    ///
    /// If `force` is true, this will attempt to reclaim memory even if the
    /// retire list is small.
    pub fn try_reclaim(&self, force: bool) -> usize {
        // Get the current set of hazardous pointers
        // This must happen atomically with respect to the retirement list processing
        let hazards = self
            .thread_hazards
            .lock()
            .expect("Failed to lock hazard list - mutex poisoned");
        let hazardous: HashSet<*mut T> = hazards.iter().map(|entry| entry.1).collect();

        // Get the retirement list
        let mut retire = self
            .retire_list
            .lock()
            .expect("Failed to lock retire list - mutex poisoned");

        // If the retire list is empty or too small and we're not forcing reclamation, do nothing
        if retire.is_empty() || (!force && retire.len() <= 5) {
            return 0;
        }

        // Separate nodes that are safe to reclaim from those that are still hazardous
        let (to_free, still_hazardous): (Vec<*mut T>, Vec<*mut T>) =
            retire.drain(..).partition(|ptr| !hazardous.contains(ptr));

        // Update the retirement list with nodes that couldn't be freed yet
        *retire = still_hazardous;

        // Count how many nodes we freed
        let freed_count = to_free.len();

        // Free the safe nodes
        for ptr in to_free {
            unsafe {
                let _ = Box::from_raw(ptr);
            }
        }

        freed_count
    }
}

impl<T> Drop for HazardPointers<T> {
    fn drop(&mut self) {
        // Final reclamation attempt to free everything
        self.try_reclaim(true);

        // If there are still pointers in the retire list, that means they're
        // still protected by some thread, which is a bug (memory leak)
        let retire = self
            .retire_list
            .lock()
            .expect("Failed to lock retire list - mutex poisoned");
        if !retire.is_empty() {
            // Just log a warning in a real application you might want to panic
            eprintln!("Warning: HazardPointers dropped with {} items still in retire list. This is a memory leak.", retire.len());
        }
    }
}

/// A node in our lock-free stack
pub struct Node<T> {
    /// The value stored in this node
    pub value: T,
    /// Pointer to the next node in the stack
    pub next: *mut Node<T>,
}

impl<T: fmt::Debug> fmt::Debug for Node<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("value", &self.value)
            .field("next", &self.next)
            .finish()
    }
}

/// A lock-free stack using hazard pointers for memory management
///
/// This implementation is thread-safe and prevents the ABA problem
/// through the use of hazard pointers.
pub struct LockFreeStack<T> {
    /// Atomic pointer to the head of the stack
    pub head: AtomicPtr<Node<T>>,
    /// Hazard pointer registry used to protect nodes from reclamation
    pub hazard_pointers: Arc<HazardPointers<Node<T>>>,
    /// Counter tracking the current size of the stack
    size: AtomicUsize,
    /// Whether to print debug information
    verbose: bool,
}

impl<T> LockFreeStack<T> {
    /// Creates a new empty stack
    pub fn new(verbose: bool) -> Self {
        LockFreeStack {
            head: AtomicPtr::new(ptr::null_mut()),
            hazard_pointers: Arc::new(HazardPointers::new()),
            size: AtomicUsize::new(0),
            verbose,
        }
    }

    /// Pushes a value onto the stack
    pub fn push(&self, value: T) -> Result<(), String> {
        // Create a new node
        let new_node = Box::into_raw(Box::new(Node {
            value,
            next: ptr::null_mut(),
        }));

        loop {
            // Get the current head with Acquire ordering to ensure we see all
            // previous writes to the stack
            let current_head = self.head.load(Ordering::Acquire);

            // Point our new node to the current head
            unsafe {
                (*new_node).next = current_head;
            }

            if self.verbose {
                println!(
                    "Attempting to push node: {:p} with next pointing to: {:p}",
                    new_node, current_head
                );
            }

            // Try to update the head to our new node
            // Release ensures previous writes are visible to other threads
            // Relaxed is used for failure case as we'll retry anyway
            match self.head.compare_exchange(
                current_head,
                new_node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // Successfully pushed the node
                    self.size.fetch_add(1, Ordering::Relaxed);
                    if self.verbose {
                        println!("Successfully pushed node: {:p}", new_node);
                    }
                    return Ok(());
                }
                Err(actual_head) => {
                    // Failed to push, try again with the updated head
                    if self.verbose {
                        println!(
                            "Push conflict detected! Expected head: {:p}, actual head: {:p}",
                            current_head, actual_head
                        );
                    }
                    unsafe {
                        (*new_node).next = actual_head;
                    }
                }
            }
        }
    }

    /// Pops a value from the stack
    pub fn pop(&self) -> Option<T> {
        loop {
            // Get the current head with Acquire ordering to ensure
            // we see all previous writes to the stack
            let current_head = self.head.load(Ordering::Acquire);
            if current_head.is_null() {
                // Stack is empty
                if self.verbose {
                    println!("Stack is empty, cannot pop");
                }
                return None;
            }

            if self.verbose {
                println!("Attempting to pop head: {:p}", current_head);
            }

            // Mark this pointer as hazardous before accessing it
            // This prevents other threads from freeing it while we're using it
            let protected_head = self.hazard_pointers.protect(current_head);

            // Check if the head has changed since we loaded it
            // This is a crucial ABA prevention step - if head changed, retry
            if self.head.load(Ordering::Acquire) != current_head {
                if self.verbose {
                    println!("Head changed during protection, retrying pop");
                }
                continue;
            }

            // Get the next node - safe because we've protected the pointer
            let next = unsafe { (*protected_head).next };

            // Try to update the head to the next node
            // Release ensures all previous writes are visible to other threads
            match self.head.compare_exchange(
                current_head,
                next,
                Ordering::Release, // Success case needs Release to make changes visible
                Ordering::Relaxed, // Failure case can be Relaxed as we'll retry anyway
            ) {
                Ok(_) => {
                    // Successfully popped the node, extract its value
                    let value = unsafe {
                        // Move out the value
                        let v = std::ptr::read(&(*protected_head).value);
                        v
                    };

                    self.size.fetch_sub(1, Ordering::Relaxed);

                    if self.verbose {
                        println!(
                            "Successfully popped head: {:p}, new head: {:p}",
                            protected_head, next
                        );
                    }

                    // Clear hazard pointer and schedule node for reclamation
                    self.hazard_pointers.clear_hazards();
                    self.hazard_pointers.retire(protected_head);

                    return Some(value);
                }
                Err(_) => {
                    // Failed to pop, retry
                    if self.verbose {
                        println!("Pop conflict detected! Head changed during CAS");
                    }
                    continue;
                }
            }
        }
    }

    /// Returns the current size of the stack
    pub fn len(&self) -> usize {
        // Relaxed is sufficient for a simple counter read
        self.size.load(Ordering::Relaxed)
    }

    /// Returns true if the stack is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Clean up resources when the stack is dropped
impl<T> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        // Pop all elements to ensure memory is freed
        while self.pop().is_some() {}

        // Final reclamation attempt
        self.hazard_pointers.try_reclaim(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_basic_operations() {
        let stack = LockFreeStack::new(false);
        assert!(stack.is_empty());

        stack.push(1).expect("Push should succeed");
        stack.push(2).expect("Push should succeed");
        stack.push(3).expect("Push should succeed");

        assert_eq!(stack.len(), 3);
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
        assert!(stack.is_empty());
    }

    #[test]
    fn test_concurrent_operations() {
        let stack = Arc::new(LockFreeStack::new(false));
        let threads = 4;
        let operations_per_thread = 100;

        let mut handles = Vec::new();

        // Push threads
        for i in 0..threads {
            let stack = Arc::clone(&stack);
            let handle = thread::spawn(move || {
                for j in 0..operations_per_thread {
                    stack
                        .push(i * operations_per_thread + j)
                        .expect("Push should succeed");
                }
            });
            handles.push(handle);
        }

        // Pop threads
        for _ in 0..threads / 2 {
            let stack = Arc::clone(&stack);
            let handle = thread::spawn(move || {
                for _ in 0..operations_per_thread {
                    let _ = stack.pop();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle
                .join()
                .expect("Thread panicked during concurrent operations");
        }

        assert_eq!(stack.len(), operations_per_thread * threads / 2);

        // Clean up remaining elements
        while stack.pop().is_some() {}
    }

    #[test]
    fn test_aba_prevention() {
        let stack = Arc::new(LockFreeStack::new(false));

        // Initial state
        stack.push(1).expect("Push should succeed");
        stack.push(2).expect("Push should succeed");

        let stack_clone1 = Arc::clone(&stack);
        let stack_clone2 = Arc::clone(&stack);

        // Thread 1: Start pop operation but get interrupted
        let handle1 = thread::spawn(move || {
            // Begin pop operation and protect head
            let head = stack_clone1.head.load(Ordering::Acquire);
            stack_clone1.hazard_pointers.protect(head);

            // Pause to allow Thread 2 to run
            thread::sleep(Duration::from_millis(100));

            // Try to complete the pop operation
            let result = stack_clone1.pop();
            stack_clone1.hazard_pointers.clear_hazards();
            result
        });

        // Thread 2: Perform operations while Thread 1 is paused
        let handle2 = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));

            // Pop both values
            let val1 = stack_clone2.pop().expect("First pop should succeed");
            let val2 = stack_clone2.pop().expect("Second pop should succeed");

            // Push them in reverse order
            stack_clone2.push(val1).expect("Push should succeed");
            stack_clone2.push(val2).expect("Push should succeed");
        });

        // Both threads should complete successfully
        let thread1_result = handle1.join().expect("Thread 1 panicked");
        handle2.join().expect("Thread 2 panicked");

        // Verify operation succeeded
        assert!(thread1_result.is_some());
    }
}
