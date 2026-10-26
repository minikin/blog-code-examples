use hazard_pointers_demo::{HazardPointers, LockFreeStack, Node};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn basic_operations() {
    let stack = LockFreeStack::new();
    stack.push(1);
    stack.push(2);
    stack.push(3);

    assert_eq!(stack.len(), 3);
    assert_eq!(stack.pop(), Some(3));
    assert_eq!(stack.pop(), Some(2));
    assert_eq!(stack.pop(), Some(1));
    assert_eq!(stack.pop(), None);
    assert!(stack.is_empty());
}

struct CountedDrop(Arc<AtomicUsize>);

impl Drop for CountedDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn popped_value_is_dropped_exactly_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    {
        let stack = LockFreeStack::new();
        stack.push(CountedDrop(Arc::clone(&drops)));
        let popped = stack.pop().expect("one value was pushed");
        stack.hazard_pointers.try_reclaim(true);
        assert_eq!(
            drops.load(Ordering::Relaxed),
            0,
            "reclaiming the node dropped the value that pop moved out"
        );
        drop(popped);
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[test]
fn values_left_on_the_stack_are_dropped_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    {
        let stack = LockFreeStack::new();
        for _ in 0..4 {
            stack.push(CountedDrop(Arc::clone(&drops)));
        }
    }
    assert_eq!(drops.load(Ordering::Relaxed), 4);
}

#[test]
fn published_pointer_is_not_reclaimed() {
    let registry: HazardPointers<Node<u32>> = HazardPointers::new();
    let node = Box::into_raw(Box::new(Node {
        value: ManuallyDrop::new(7),
        next: AtomicPtr::new(std::ptr::null_mut()),
    }));

    let guard = registry.protect(node);
    unsafe { registry.retire(node) };
    assert_eq!(registry.try_reclaim(true), 0);
    assert_eq!(registry.retired_len(), 1);

    drop(guard);
    assert_eq!(registry.try_reclaim(true), 1);
    assert_eq!(registry.retired_len(), 0);
}

#[test]
fn concurrent_push_and_pop() {
    const THREADS: usize = 4;
    const OPS: usize = 64;

    let stack = Arc::new(LockFreeStack::new());
    let barrier = Arc::new(Barrier::new(THREADS));
    let popped = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let stack = Arc::clone(&stack);
            let barrier = Arc::clone(&barrier);
            let popped = Arc::clone(&popped);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS {
                    stack.push(t * OPS + i);
                    if stack.pop().is_some() {
                        popped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("worker panicked");
    }

    let left = THREADS * OPS - popped.load(Ordering::Relaxed);
    assert_eq!(stack.len(), left);
    let mut drained = 0;
    while stack.pop().is_some() {
        drained += 1;
    }
    assert_eq!(drained, left);
}
