use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};
use std::mem::MaybeUninit;
use std::sync::atomic::Ordering;

/// Error types for queue operations
#[derive(Debug, PartialEq, Eq)]
pub enum QueueError {
    /// Returned when trying to dequeue from an empty queue
    Empty,
}

/// A node in the lock-free queue
struct Node<T> {
    /// Uninitialized in the sentinel. `dequeue` moves the value out of the node
    /// that becomes the new sentinel, so no node ever drops its own value:
    /// `Drop for LockFreeQueue` drops the values still in the queue.
    value: MaybeUninit<T>,
    /// Pointer to the next node in the queue
    next: Atomic<Node<T>>,
}

/// A lock-free queue implementation using epoch-based memory reclamation
///
/// # Type Parameters
///
/// * `T`: The type of elements in the queue. Must be Send + Sync for thread-safety
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
            value: MaybeUninit::uninit(),
            next: Atomic::null(),
        });
        // SAFETY: the queue is not shared yet, so no other thread can observe
        // the sentinel.
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
            value: MaybeUninit::new(value),
            next: Atomic::null(),
        })
        .into_shared(&guard);

        loop {
            let tail = self.tail.load(Ordering::Acquire, &guard);
            // SAFETY: the tail is never null, and the guard keeps it allocated.
            let tail_ref = unsafe { tail.deref() };
            let next = tail_ref.next.load(Ordering::Acquire, &guard);

            if next.is_null() {
                if tail_ref
                    .next
                    .compare_exchange(
                        Shared::null(),
                        new_node,
                        Ordering::Release,
                        Ordering::Relaxed,
                        &guard,
                    )
                    .is_ok()
                {
                    let _ = self.tail.compare_exchange(
                        tail,
                        new_node,
                        Ordering::Release,
                        Ordering::Relaxed,
                        &guard,
                    );
                    return;
                }
            } else {
                // Another enqueue linked its node but has not moved the tail yet.
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
            let head = self.head.load(Ordering::Acquire, &guard);
            // SAFETY: the head is never null, and the guard keeps it allocated.
            let next = unsafe { head.deref() }.next.load(Ordering::Acquire, &guard);
            if next.is_null() {
                return Err(QueueError::Empty);
            }

            // Move the tail past the old head before retiring it, or the tail
            // would point at freed memory.
            let tail = self.tail.load(Ordering::Acquire, &guard);
            if tail == head {
                let _ = self.tail.compare_exchange(
                    tail,
                    next,
                    Ordering::Release,
                    Ordering::Relaxed,
                    &guard,
                );
                continue;
            }

            if self
                .head
                .compare_exchange(head, next, Ordering::AcqRel, Ordering::Acquire, &guard)
                .is_ok()
            {
                // SAFETY: winning the CAS makes `next` the new sentinel, and only
                // the winner reads its value. The value was initialized before the
                // node was published, and nothing drops it from the node.
                let value = unsafe { next.deref().value.assume_init_read() };
                // SAFETY: the old head is unlinked, and only the CAS winner retires it.
                unsafe { guard.defer_destroy(head) };
                return Ok(value);
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
        let head = self.head.load(Ordering::Acquire, &guard);
        // SAFETY: the head is never null, and the guard keeps it allocated.
        unsafe { head.deref() }
            .next
            .load(Ordering::Acquire, &guard)
            .is_null()
    }
}

impl<T: Send + Sync + 'static> Default for LockFreeQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for LockFreeQueue<T> {
    fn drop(&mut self) {
        // SAFETY: `&mut self` means no other thread can reach any node, so they
        // can be freed immediately. The first node is the sentinel, whose value
        // is uninitialized or already moved out. Every later node still owns
        // its value.
        unsafe {
            let guard = epoch::unprotected();
            let mut current = self.head.load(Ordering::Relaxed, guard);
            let mut is_sentinel = true;
            while !current.is_null() {
                let mut node = current.into_owned();
                current = node.next.load(Ordering::Relaxed, guard);
                if !is_sentinel {
                    node.value.assume_init_drop();
                }
                is_sentinel = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
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
            let queue = LockFreeQueue::new();
            for i in 0..16 {
                queue.enqueue(CountsDrops(i));
            }
            for _ in 0..8 {
                drop(queue.dequeue().unwrap());
            }
            // The remaining 8 go through Drop for LockFreeQueue.
        }

        // Pin repeatedly so the global epoch advances far enough to free the
        // retired sentinels. They must not drop a value a second time.
        for _ in 0..512 {
            epoch::pin().flush();
        }

        assert_eq!(DROPS.load(Ordering::Relaxed), 16);
    }

    #[test]
    fn test_queue_concurrent_operations() {
        let queue = Arc::new(LockFreeQueue::new());
        let producers = 5;
        let per_producer = if cfg!(miri) { 20 } else { 100 };
        let total = producers * per_producer;
        let received = Arc::new(AtomicUsize::new(0));

        let producer_handles: Vec<_> = (0..producers)
            .map(|i| {
                let queue = Arc::clone(&queue);
                thread::spawn(move || {
                    for j in 0..per_producer {
                        queue.enqueue(i * per_producer + j);
                    }
                })
            })
            .collect();

        let consumer_handles: Vec<_> = (0..3)
            .map(|_| {
                let queue = Arc::clone(&queue);
                let received = Arc::clone(&received);
                thread::spawn(move || {
                    let mut mine = Vec::new();
                    while received.load(Ordering::Relaxed) < total {
                        match queue.dequeue() {
                            Ok(value) => {
                                mine.push(value);
                                received.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(QueueError::Empty) => thread::yield_now(),
                        }
                    }
                    mine
                })
            })
            .collect();

        for handle in producer_handles {
            handle.join().unwrap();
        }
        let mut all: Vec<_> = consumer_handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();

        // No dedup: a value dequeued twice must fail this comparison.
        all.sort_unstable();
        assert_eq!(all, (0..total).collect::<Vec<_>>());
        assert!(queue.is_empty());
    }
}
