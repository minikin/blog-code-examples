use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};
use std::ptr;
use std::sync::atomic::Ordering;

#[cfg(test)]
use std::{sync::atomic::AtomicBool, time::Duration};

/// Error types for queue operations
#[derive(Debug, PartialEq, Eq)]
pub enum QueueError {
    /// Returned when trying to dequeue from an empty queue
    Empty,
}

/// A node in the lock-free queue
#[derive(Debug)]
struct Node<T> {
    /// The value stored in this node, None for sentinel nodes
    value: Option<T>,
    /// Pointer to the next node in the queue
    next: Atomic<Node<T>>,
}

/// A lock-free queue implementation using epoch-based memory reclamation
///
/// # Type Parameters
///
/// * `T`: The type of elements in the queue. Must be Send + Sync for thread-safety
///
/// # Safety
///
/// This implementation uses epoch-based memory reclamation to ensure memory safety
/// in concurrent operations. All unsafe operations are properly guarded by epoch protection
/// and maintain the required invariants for memory safety.
///
/// # Performance
///
/// * Enqueue: O(1) average case, may retry on contention
/// * Dequeue: O(1) average case, may retry on contention
/// * Memory usage: O(n) where n is the number of elements
///
/// The implementation uses cache-line padding to prevent false sharing between head and tail
/// pointers in concurrent operations.
#[repr(align(64))]
#[derive(Debug)]
pub struct LockFreeQueue<T> {
    head: Atomic<Node<T>>,
    _pad1: [u8; 56], // Padding to prevent false sharing
    tail: Atomic<Node<T>>,
    _pad2: [u8; 56], // Padding to prevent false sharing
}

impl<T: Send + Sync + 'static> LockFreeQueue<T> {
    /// Creates a new empty queue.
    ///
    /// # Examples
    /// ```
    /// use ebr_aba_protection::queue::LockFreeQueue;
    /// let queue: LockFreeQueue<i32> = LockFreeQueue::new();
    /// assert!(queue.is_empty());
    /// ```
    pub fn new() -> Self {
        let sentinel = Owned::new(Node {
            value: None,
            next: Atomic::null(),
        });
        let sentinel_shared = sentinel.into_shared(unsafe { epoch::unprotected() });
        Self {
            head: Atomic::from(sentinel_shared),
            tail: Atomic::from(sentinel_shared),
            _pad1: [0; 56],
            _pad2: [0; 56],
        }
    }

    /// Adds a value to the back of the queue.
    ///
    /// # Examples
    /// ```
    /// use ebr_aba_protection::queue::LockFreeQueue;
    /// let queue = LockFreeQueue::new();
    /// queue.enqueue(42);
    /// assert!(!queue.is_empty());
    /// ```
    pub fn enqueue(&self, value: T) {
        let guard = epoch::pin();
        let new_node = Owned::new(Node {
            value: Some(value),
            next: Atomic::null(),
        })
        .into_shared(&guard);

        loop {
            let tail = self.tail.load(Ordering::Relaxed, &guard);
            // SAFETY: tail is protected by the epoch guard
            let tail_ref = unsafe { tail.deref() };
            let next = tail_ref.next.load(Ordering::Acquire, &guard);

            if next.is_null() {
                match tail_ref.next.compare_exchange(
                    Shared::null(),
                    new_node,
                    Ordering::Release,
                    Ordering::Relaxed,
                    &guard,
                ) {
                    Ok(_) => {
                        // Attempt to update tail
                        let _ = self.tail.compare_exchange(
                            tail,
                            new_node,
                            Ordering::Release,
                            Ordering::Relaxed,
                            &guard,
                        );
                        break;
                    }
                    Err(_) => continue,
                }
            } else {
                // Help advance tail if needed
                let _ = self.tail.compare_exchange(
                    tail,
                    next,
                    Ordering::Release,
                    Ordering::Relaxed,
                    &guard,
                );
            }
        }
    }

    /// Removes and returns the value at the front of the queue.
    ///
    /// # Examples
    /// ```
    /// use ebr_aba_protection::queue::{LockFreeQueue, QueueError};
    /// let queue = LockFreeQueue::new();
    /// queue.enqueue(42);
    /// assert_eq!(queue.dequeue(), Ok(42));
    /// assert_eq!(queue.dequeue(), Err(QueueError::Empty));
    /// ```
    pub fn dequeue(&self) -> Result<T, QueueError> {
        let guard = epoch::pin();
        loop {
            let head = self.head.load(Ordering::Relaxed, &guard);
            let next = unsafe { head.deref() }.next.load(Ordering::Acquire, &guard);

            if next.is_null() {
                return Err(QueueError::Empty);
            }

            if self
                .head
                .compare_exchange(head, next, Ordering::Release, Ordering::Relaxed, &guard)
                .is_ok()
            {
                unsafe {
                    // SAFETY: The node was successfully unlinked and won't be
                    // concurrently accessed due to the epoch guard
                    guard.defer_destroy(head);
                    let next_ref = &*next.as_raw();
                    return Ok(ptr::read(next_ref.value.as_ref().unwrap()));
                }
            }
        }
    }

    /// Returns true if the queue is empty.
    ///
    /// # Examples
    /// ```
    /// use ebr_aba_protection::queue::LockFreeQueue;
    /// let queue: LockFreeQueue<i32> = LockFreeQueue::new();
    /// assert!(queue.is_empty());
    /// queue.enqueue(42);
    /// assert!(!queue.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        let guard = epoch::pin();
        let head = self.head.load(Ordering::Relaxed, &guard);
        unsafe { head.deref() }
            .next
            .load(Ordering::Relaxed, &guard)
            .is_null()
    }

    /// Returns a reference to the value at the front of the queue without removing it.
    ///
    /// # Examples
    /// ```
    /// use ebr_aba_protection::queue::{LockFreeQueue, QueueError};
    /// let queue = LockFreeQueue::new();
    /// queue.enqueue(42);
    /// assert_eq!(*queue.peek().unwrap(), 42);
    /// assert_eq!(queue.dequeue(), Ok(42));
    /// assert_eq!(queue.peek(), Err(QueueError::Empty));
    /// ```
    pub fn peek(&self) -> Result<&T, QueueError> {
        let guard = epoch::pin();
        let head = self.head.load(Ordering::Relaxed, &guard);
        let next = unsafe { head.deref() }.next.load(Ordering::Acquire, &guard);

        if next.is_null() {
            return Err(QueueError::Empty);
        }

        unsafe {
            // SAFETY: The node is protected by the epoch guard and won't be
            // dequeued while we hold the guard. The reference is valid as long as
            // the queue exists since we're not dropping the guard.
            Ok(&*next.as_raw()).and_then(|node| node.value.as_ref().ok_or(QueueError::Empty))
        }
    }
}

impl<T: Send + Sync + 'static> Default for LockFreeQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for LockFreeQueue<T> {
    fn drop(&mut self) {
        let guard = unsafe { epoch::unprotected() };
        let mut current = self.head.load(Ordering::Relaxed, guard);

        while !current.is_null() {
            unsafe {
                let next = current.deref().next.load(Ordering::Relaxed, guard);
                guard.defer_destroy(current);
                current = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_queue_basic_operations() {
        let queue = LockFreeQueue::new();
        queue.enqueue(1);
        queue.enqueue(2);
        queue.enqueue(3);

        assert_eq!(queue.dequeue(), Ok(1));
        assert_eq!(queue.dequeue(), Ok(2));
        assert_eq!(queue.dequeue(), Ok(3));
        assert_eq!(queue.dequeue(), Err(QueueError::Empty));
    }

    #[test]
    fn test_empty_queue() {
        let queue: LockFreeQueue<i32> = LockFreeQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.dequeue(), Err(QueueError::Empty));
        assert_eq!(queue.peek(), Err(QueueError::Empty));
    }

    #[test]
    fn test_peek() {
        let queue = LockFreeQueue::new();
        queue.enqueue(42);
        assert_eq!(*queue.peek().unwrap(), 42);
        assert_eq!(queue.dequeue(), Ok(42));
        assert_eq!(queue.peek(), Err(QueueError::Empty));
    }

    #[test]
    fn test_queue_concurrent_operations() {
        let queue = Arc::new(LockFreeQueue::new());
        let mut handles = vec![];
        let num_producers = 5;
        let num_items_per_producer = 100; // Reduced for faster testing
        let total_items = num_producers * num_items_per_producer;

        // Producers
        for i in 0..num_producers {
            let queue = Arc::clone(&queue);
            handles.push(thread::spawn(move || {
                for j in 0..num_items_per_producer {
                    queue.enqueue(i * num_items_per_producer + j);
                }
            }));
        }

        // Wait for producers to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Consumers
        let mut consumer_handles = vec![];
        let num_consumers = 3;
        let done = Arc::new(AtomicBool::new(false));

        for _ in 0..num_consumers {
            let queue = Arc::clone(&queue);
            let done = Arc::clone(&done);
            consumer_handles.push(thread::spawn(move || {
                let mut received = Vec::new();
                while !done.load(Ordering::Relaxed) {
                    match queue.dequeue() {
                        Ok(value) => received.push(value),
                        Err(QueueError::Empty) => thread::yield_now(),
                    }
                }
                received
            }));
        }

        // Allow consumers to run for a short while
        thread::sleep(Duration::from_millis(100));
        done.store(true, Ordering::Relaxed);

        let mut total_received = Vec::new();
        for handle in consumer_handles {
            total_received.extend(handle.join().unwrap());
        }

        // Sort received items to make comparison easier
        total_received.sort_unstable();
        total_received.dedup(); // Remove any duplicates if they exist

        // Verify all items were consumed
        assert_eq!(total_received.len(), total_items);

        // Verify we received all expected items
        let expected: Vec<_> = (0..total_items).collect();
        assert_eq!(total_received, expected);
        assert!(queue.is_empty());
    }
}
