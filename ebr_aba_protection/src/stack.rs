use crossbeam_epoch::{self as epoch, Atomic, Owned};
use crossbeam_utils::Backoff;
use std::fmt::Debug;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// Error types that can occur during stack operations
#[derive(Debug, PartialEq)]
pub enum StackError {
    /// Indicates that the stack has reached its maximum capacity
    CapacityExceeded,
    /// Indicates that the push operation failed after maximum retries
    PushFailed,
}

/// A node in the lock-free stack
///
/// Each node contains a value and an atomic pointer to the next node.
struct Node<T> {
    /// The value stored in this node
    value: T,
    /// Atomic pointer to the next node in the stack
    next: Atomic<Node<T>>,
}

/// A lock-free stack implementation using epoch-based memory reclamation
///
/// This implementation provides O(1) push and pop operations with strong
/// ABA prevention through epoch-based garbage collection.
///
/// # Type Parameters
/// * `T`: The type of values stored in the stack
///
/// # Examples
/// ```
/// use ebr_aba_protection::LockFreeStack;
///
/// let stack = LockFreeStack::new();
/// stack.push(1).unwrap();
/// assert_eq!(stack.pop(), Some(1));
/// ```
#[derive(Debug)]
pub struct LockFreeStack<T: Send + Sync + 'static> {
    head: Atomic<Node<T>>,
    size: AtomicUsize,
    capacity: Option<usize>,
}

impl<T: Send + Sync + 'static> Default for LockFreeStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + Sync + 'static> LockFreeStack<T> {
    /// Creates a new empty stack with unlimited capacity
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            size: AtomicUsize::new(0),
            capacity: None,
        }
    }

    /// Creates a new empty stack with specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            head: Atomic::null(),
            size: AtomicUsize::new(0),
            capacity: Some(capacity),
        }
    }

    /// Pushes a value onto the stack
    ///
    /// # Arguments
    /// * `value`: The value to push onto the stack
    ///
    /// # Returns
    /// * `Ok(())` if the push was successful
    /// * `Err(StackError::CapacityExceeded)` if the stack is at capacity
    /// * `Err(StackError::PushFailed)` if the push failed after maximum retries
    ///
    /// # Safety
    /// This operation is lock-free and thread-safe.
    pub fn push(&self, value: T) -> Result<(), StackError> {
        // Check capacity if set
        if let Some(capacity) = self.capacity {
            if self.size.load(Ordering::Relaxed) >= capacity {
                return Err(StackError::CapacityExceeded);
            }
        }

        let guard = epoch::pin();
        let node = Owned::new(Node {
            value,
            next: Atomic::null(),
        })
        .into_shared(&guard);

        let backoff = Backoff::new();
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 1000;

        loop {
            let head = self.head.load(Ordering::Relaxed, &guard);
            unsafe {
                (*node.as_raw()).next.store(head, Ordering::Release);
            }

            match self.head.compare_exchange(
                head,
                node,
                Ordering::AcqRel,
                Ordering::Acquire,
                &guard,
            ) {
                Ok(_) => {
                    self.size.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        return Err(StackError::PushFailed);
                    }
                    backoff.spin();
                    if backoff.is_completed() {
                        thread::yield_now();
                    }
                }
            }
        }
    }

    /// Removes and returns the top element from the stack
    ///
    /// # Returns
    /// * `Some(T)` if the stack was not empty
    /// * `None` if the stack was empty
    ///
    /// # Safety
    /// This operation is lock-free and thread-safe.
    pub fn pop(&self) -> Option<T> {
        let guard = epoch::pin();
        let backoff = Backoff::new();
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 1000;

        loop {
            let head = self.head.load(Ordering::Acquire, &guard);
            match unsafe { head.as_ref() } {
                Some(head_node) => {
                    let next = head_node.next.load(Ordering::Acquire, &guard);
                    if self
                        .head
                        .compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire, &guard)
                        .is_ok()
                    {
                        self.size.fetch_sub(1, Ordering::Relaxed);
                        unsafe {
                            guard.defer_destroy(head);
                            return Some(ptr::read(&(*head.as_raw()).value));
                        }
                    }
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        // If we've failed too many times, back off and try again
                        thread::yield_now();
                        attempts = 0;
                    }
                    backoff.spin();
                }
                None => return None,
            }
        }
    }

    /// Returns the current size of the stack
    ///
    /// Note: Due to concurrent operations, the size may change
    /// immediately after this call returns.
    pub fn len(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// Returns true if the stack is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Attempts to collect garbage from previous operations
    ///
    /// This is an optimization that can be called periodically to
    /// help manage memory usage.
    pub fn try_collect_garbage(&self) {
        let mut guard = epoch::pin();
        guard.flush();
        guard.repin();
        guard.flush();
    }
}

impl<T: Send + Sync + 'static> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_epoch::Shared;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_stack_basic_operations() {
        let stack = LockFreeStack::new();
        assert!(stack.is_empty());

        stack.push(1).unwrap();
        stack.push(2).unwrap();
        stack.push(3).unwrap();

        assert_eq!(stack.len(), 3);
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_stack_capacity() {
        let stack = LockFreeStack::with_capacity(2);

        assert!(stack.push(1).is_ok());
        assert!(stack.push(2).is_ok());
        assert_eq!(stack.push(3), Err(StackError::CapacityExceeded));

        assert_eq!(stack.pop(), Some(2));
        assert!(stack.push(3).is_ok());
    }

    #[test]
    fn test_stack_concurrent_operations() {
        let stack = Arc::new(LockFreeStack::new());
        let mut handles = vec![];

        // Spawn multiple push threads
        for i in 0..1000 {
            let stack = Arc::clone(&stack);
            handles.push(thread::spawn(move || {
                stack.push(i).unwrap();
            }));
        }

        // Spawn multiple pop threads
        for _ in 0..500 {
            let stack = Arc::clone(&stack);
            handles.push(thread::spawn(move || {
                stack.pop();
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(stack.len(), 500);
    }

    #[test]
    fn test_stack_concurrent_mixed_operations() {
        let stack = Arc::new(LockFreeStack::new());
        let mut handles = vec![];

        for i in 0..10 {
            let stack = Arc::clone(&stack);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    if j % 2 == 0 {
                        stack.push(i * 100 + j).unwrap();
                    } else {
                        stack.pop();
                    }
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_aba_prevention() {
        let stack = Arc::new(LockFreeStack::new());

        // Push initial values
        stack.push(1).unwrap();
        stack.push(2).unwrap();

        let stack_clone = stack.clone();

        // Thread 1: Try to pop and modify
        let t1 = thread::spawn(move || {
            let guard = epoch::pin();
            let old_head = stack_clone.head.load(Ordering::Acquire, &guard);
            thread::sleep(Duration::from_millis(100));

            stack_clone
                .head
                .compare_exchange(
                    old_head,
                    Shared::null(),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    &guard,
                )
                .is_err()
        });

        // Thread 2: Modify the stack
        thread::sleep(Duration::from_millis(50));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        stack.push(3).unwrap();

        assert!(t1.join().unwrap());
    }

    #[test]
    fn test_garbage_collection() {
        let stack = LockFreeStack::new();

        // Push and pop many times to create garbage
        for i in 0..1000 {
            stack.push(i).unwrap();
        }
        for _ in 0..1000 {
            stack.pop();
        }

        // Try to collect garbage
        stack.try_collect_garbage();

        // Verify stack is still usable
        stack.push(42).unwrap();
        assert_eq!(stack.pop(), Some(42));
    }
}
