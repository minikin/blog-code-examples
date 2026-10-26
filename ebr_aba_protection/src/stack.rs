use crossbeam_epoch::{self as epoch, Atomic, Owned};
use crossbeam_utils::Backoff;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Error returned by [`LockFreeStack::push`].
#[derive(Debug, PartialEq, Eq)]
pub enum StackError {
    /// The stack was created with [`LockFreeStack::with_capacity`] and is full.
    CapacityExceeded,
}

/// A node in the lock-free stack.
///
/// `value` is a `ManuallyDrop<T>` because `pop` moves the value out of the node
/// and then hands the node itself to `defer_destroy`. Without `ManuallyDrop` the
/// deferred destructor would drop a value the caller already owns.
struct Node<T> {
    value: ManuallyDrop<T>,
    next: Atomic<Node<T>>,
}

/// A lock-free stack that reclaims nodes through crossbeam's epoch-based reclamation.
///
/// `push` and `pop` are O(1) amortized and never free a node another thread can
/// still reach, which is what rules out ABA: a popped address cannot come back
/// from the allocator while any thread is pinned in an epoch that could observe it.
///
/// # Examples
///
/// ~~~
/// use ebr_aba_protection::LockFreeStack;
///
/// let stack = LockFreeStack::new();
/// stack.push(1).unwrap();
/// assert_eq!(stack.pop(), Some(1));
/// ~~~
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
    /// Creates an empty stack with no capacity limit.
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            size: AtomicUsize::new(0),
            capacity: None,
        }
    }

    /// Creates an empty stack that holds at most `capacity` values.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            head: Atomic::null(),
            size: AtomicUsize::new(0),
            capacity: Some(capacity),
        }
    }

    /// Pushes a value onto the stack.
    ///
    /// Returns `Err(StackError::CapacityExceeded)` if the stack was created with
    /// a capacity and is full. The CAS loop itself never gives up, so there is no
    /// failure mode for contention.
    pub fn push(&self, value: T) -> Result<(), StackError> {
        // Claim the slot before allocating, so a capacity is a real bound and not
        // a check that every concurrent pusher can pass at once.
        self.reserve_slot()?;

        let guard = epoch::pin();
        let mut node = Owned::new(Node {
            value: ManuallyDrop::new(value),
            next: Atomic::null(),
        });

        let backoff = Backoff::new();
        loop {
            let head = self.head.load(Ordering::Relaxed, &guard);
            node.next.store(head, Ordering::Relaxed);

            match self.head.compare_exchange(
                head,
                node,
                Ordering::Release,
                Ordering::Relaxed,
                &guard,
            ) {
                Ok(_) => return Ok(()),
                Err(failed) => {
                    // The node is still ours, so a retry cannot leak it.
                    node = failed.new;
                    backoff.spin();
                }
            }
        }
    }

    fn reserve_slot(&self) -> Result<(), StackError> {
        let Some(capacity) = self.capacity else {
            self.size.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        };

        let mut size = self.size.load(Ordering::Relaxed);
        loop {
            if size >= capacity {
                return Err(StackError::CapacityExceeded);
            }
            match self.size.compare_exchange_weak(
                size,
                size + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => size = actual,
            }
        }
    }

    /// Removes and returns the top value, or `None` if the stack is empty.
    pub fn pop(&self) -> Option<T> {
        let guard = epoch::pin();
        let backoff = Backoff::new();

        loop {
            let head = self.head.load(Ordering::Acquire, &guard);
            let head_node = unsafe { head.as_ref() }?;

            // Reading `next` is sound even if another thread has already unlinked
            // this node: our guard keeps it allocated for as long as we are pinned.
            let next = head_node.next.load(Ordering::Acquire, &guard);

            if self
                .head
                .compare_exchange(head, next, Ordering::Release, Ordering::Relaxed, &guard)
                .is_ok()
            {
                self.size.fetch_sub(1, Ordering::Relaxed);
                unsafe {
                    // We won the CAS, so no other thread will take this value.
                    let value = ManuallyDrop::take(&mut (*head.as_raw().cast_mut()).value);
                    guard.defer_destroy(head);
                    return Some(value);
                }
            }

            backoff.spin();
        }
    }

    /// Returns the number of values in the stack, including slots reserved by a
    /// `push` that has not linked its node yet.
    pub fn len(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// Returns true if the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Seals this thread's bag of deferred nodes into the global queue and tries
    /// to advance the epoch once.
    ///
    /// This does not promise that anything is freed. It only brings forward work
    /// that `pin` would otherwise do on some later call.
    pub fn try_collect_garbage(&self) {
        epoch::pin().flush();
    }
}

impl<T: Send + Sync + 'static> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
        // The drained nodes are still in this thread's bag. Hand them over so
        // they are not held until the thread happens to pin 64 more times.
        epoch::pin().flush();
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
    fn test_values_are_dropped_exactly_once() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);

        struct CountsDrops(#[allow(dead_code)] u32);

        impl Drop for CountsDrops {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::Relaxed);
            }
        }

        {
            let stack = LockFreeStack::new();
            for i in 0..16 {
                stack.push(CountsDrops(i)).unwrap();
            }
            for _ in 0..8 {
                drop(stack.pop().unwrap());
            }
            // The remaining 8 go through Drop for LockFreeStack.
        }

        // Pin repeatedly so the global epoch advances far enough to run the
        // deferred destructors of the nodes we popped.
        for _ in 0..512 {
            epoch::pin().flush();
        }

        assert_eq!(DROPS.load(Ordering::Relaxed), 16);
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

    // Capacity needs real contention. A load-then-compare check passes every
    // single-threaded test and still lets 32 concurrent pushers all through.
    #[test]
    fn test_capacity_holds_under_contention() {
        let stack = Arc::new(LockFreeStack::with_capacity(8));
        let accepted = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..32)
            .map(|i| {
                let stack = Arc::clone(&stack);
                let accepted = Arc::clone(&accepted);
                thread::spawn(move || {
                    if stack.push(i).is_ok() {
                        accepted.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(accepted.load(Ordering::Relaxed), 8);
        assert_eq!(stack.len(), 8);
    }

    #[test]
    fn test_stack_concurrent_operations() {
        let stack = Arc::new(LockFreeStack::new());
        let mut handles = vec![];

        for i in 0..1000 {
            let stack = Arc::clone(&stack);
            handles.push(thread::spawn(move || {
                stack.push(i).unwrap();
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(stack.len(), 1000);

        // The poppers start only once every push is visible. Overlapping them
        // makes the final count unpredictable: a pop that finds the stack
        // momentarily empty returns None without decrementing.
        let mut handles = vec![];
        for _ in 0..500 {
            let stack = Arc::clone(&stack);
            handles.push(thread::spawn(move || {
                stack.pop().expect("1000 pushes are already visible")
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
        stack.push(1).unwrap();
        stack.push(2).unwrap();

        let stack_clone = Arc::clone(&stack);

        let t1 = thread::spawn(move || {
            let guard = epoch::pin();
            let old_head = stack_clone.head.load(Ordering::Acquire, &guard);
            thread::sleep(Duration::from_millis(100));

            // While this thread stays pinned, the node behind `old_head` cannot
            // be freed, so the allocator cannot hand its address back out. The
            // CAS fails instead of succeeding against a recycled node.
            stack_clone
                .head
                .compare_exchange(
                    old_head,
                    Shared::null(),
                    Ordering::Release,
                    Ordering::Relaxed,
                    &guard,
                )
                .is_err()
        });

        thread::sleep(Duration::from_millis(50));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        stack.push(3).unwrap();

        assert!(t1.join().unwrap());
    }

    #[test]
    fn test_garbage_collection() {
        let stack = LockFreeStack::new();

        let rounds = if cfg!(miri) { 20 } else { 1000 };
        for i in 0..rounds {
            stack.push(i).unwrap();
        }
        for _ in 0..rounds {
            stack.pop().unwrap();
        }

        stack.try_collect_garbage();

        stack.push(42).unwrap();
        assert_eq!(stack.pop(), Some(42));
    }
}
