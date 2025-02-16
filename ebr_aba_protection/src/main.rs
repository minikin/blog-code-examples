use crossbeam_epoch::{self as epoch, Atomic, Owned, Shared};
use crossbeam_utils::Backoff;
use std::fmt::Debug;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub enum StackError {
    PushFailed,
}

/// A node in our lock-free stack
#[derive(Debug)]
struct Node<T> {
    /// The value stored in this node
    value: T,
    /// Atomic pointer to the next node
    next: Atomic<Node<T>>,
}

/// A lock-free stack implementation using epoch-based memory reclamation
#[derive(Debug)]
pub struct LockFreeStack<T> {
    head: Atomic<Node<T>>,
    size: AtomicUsize,
}

impl<T: Send + Sync + Debug + 'static> LockFreeStack<T> {
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            size: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, value: T) -> Result<(), StackError> {
        let guard = epoch::pin();
        let node = Owned::new(Node {
            value,
            next: Atomic::null(),
        })
        .into_shared(&guard);

        let backoff = Backoff::new();
        loop {
            let head = self.head.load(Ordering::Acquire, &guard);
            unsafe {
                (*node.as_raw()).next.store(head, Ordering::Release);
            }

            match self.head.compare_exchange(
                head,
                node,
                Ordering::Release,
                Ordering::Relaxed,
                &guard,
            ) {
                Ok(_) => {
                    self.size.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(_) => backoff.spin(),
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        let guard = epoch::pin();
        let backoff = Backoff::new();

        loop {
            let head = self.head.load(Ordering::Acquire, &guard);
            match unsafe { head.as_ref() } {
                Some(head_node) => {
                    let next = head_node.next.load(Ordering::Acquire, &guard);
                    if self
                        .head
                        .compare_exchange(head, next, Ordering::Release, Ordering::Relaxed, &guard)
                        .is_ok()
                    {
                        self.size.fetch_sub(1, Ordering::Relaxed);
                        unsafe {
                            guard.defer_destroy(head);
                            return Some(ptr::read(&(*head.as_raw()).value));
                        }
                    }
                    backoff.spin();
                }
                None => return None,
            }
        }
    }

    pub fn len(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Queue implementation
#[derive(Debug)]
struct QueueNode<T> {
    value: Option<T>,
    next: Atomic<QueueNode<T>>,
}

#[derive(Debug)]
pub struct LockFreeQueue<T> {
    head: Atomic<QueueNode<T>>,
    tail: Atomic<QueueNode<T>>,
}

impl<T: Send + Sync + Debug + 'static> LockFreeQueue<T> {
    pub fn new() -> Self {
        let sentinel = Owned::new(QueueNode {
            value: None,
            next: Atomic::null(),
        });
        let sentinel_shared = sentinel.into_shared(unsafe { &epoch::unprotected() });
        Self {
            head: Atomic::from(sentinel_shared),
            tail: Atomic::from(sentinel_shared),
        }
    }

    pub fn enqueue(&self, value: T) {
        let guard = epoch::pin();
        let new_node = Owned::new(QueueNode {
            value: Some(value),
            next: Atomic::null(),
        })
        .into_shared(&guard);

        loop {
            let tail = self.tail.load(Ordering::Acquire, &guard);
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

    pub fn dequeue(&self) -> Option<T> {
        let guard = epoch::pin();
        loop {
            let head = self.head.load(Ordering::Acquire, &guard);
            let next = unsafe { head.deref() }.next.load(Ordering::Acquire, &guard);

            if next.is_null() {
                return None;
            }

            if self
                .head
                .compare_exchange(head, next, Ordering::Release, Ordering::Relaxed, &guard)
                .is_ok()
            {
                unsafe {
                    guard.defer_destroy(head);
                    let next_ref = &*next.as_raw();
                    // Use ptr::read to move out the value without requiring mutable access
                    return next_ref.value.as_ref().map(|value| ptr::read(value));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_queue_basic_operations() {
        let queue = LockFreeQueue::new();
        queue.enqueue(1);
        queue.enqueue(2);
        queue.enqueue(3);

        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.dequeue(), None);
    }

    #[test]
    fn test_stack_concurrent_operations() {
        let stack = Arc::new(LockFreeStack::new());
        let mut handles = vec![];

        for i in 0..10 {
            let stack = Arc::clone(&stack);
            handles.push(thread::spawn(move || {
                stack.push(i).unwrap();
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(stack.len(), 10);
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
            thread::sleep(Duration::from_millis(100)); // Simulate suspension

            // Try to modify using the old head - this should fail
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

        // Thread 2: Successfully modify the stack
        thread::sleep(Duration::from_millis(50));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        stack.push(3).unwrap();

        // Thread 1's CAS should fail
        assert!(t1.join().unwrap());
    }
}

fn main() {
    println!("Running epoch-based reclamation examples...");

    // Basic stack demo
    let stack = LockFreeStack::new();
    stack.push(1).unwrap();
    stack.push(2).unwrap();
    println!("Stack size: {}", stack.len());
    println!("Popped: {:?}", stack.pop());

    // Basic queue demo
    let queue = LockFreeQueue::new();
    queue.enqueue(1);
    queue.enqueue(2);
    println!("Dequeued: {:?}", queue.dequeue());
}
