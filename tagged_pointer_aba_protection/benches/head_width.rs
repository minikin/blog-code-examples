//! The table in part 1: the pooled stack with its tagged 128-bit head against
//! the same stack with plain `AtomicPtr` heads, each thread doing a push and
//! then a pop.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

// Included by path so the tagged stack is exactly the code the post shows.
#[allow(dead_code, unused_imports)]
#[path = "../src/main.rs"]
mod tagged;

use tagged::LockFreeStack;

/// The pooled stack with plain pointers for the head and the free list.
///
/// This version is ABA-prone by design: a stale head can win its CAS, so nodes
/// can be lost or handed out twice under contention. It exists only to price
/// the 64-bit CAS against the 128-bit one. `value` is atomic so a node handed
/// out twice is a wrong answer and not a data race, and nodes are freed only
/// in `Drop`, so nothing is read after it is freed.
struct PlainStack {
    head: AtomicPtr<PlainNode>,
    free: AtomicPtr<PlainNode>,
    all: AtomicPtr<PlainNode>,
}

struct PlainNode {
    value: AtomicI32,
    next: AtomicPtr<PlainNode>,
    all_next: AtomicPtr<PlainNode>,
}

impl PlainStack {
    fn new() -> Self {
        PlainStack {
            head: AtomicPtr::new(ptr::null_mut()),
            free: AtomicPtr::new(ptr::null_mut()),
            all: AtomicPtr::new(ptr::null_mut()),
        }
    }

    fn alloc(&self, value: i32) -> *mut PlainNode {
        loop {
            let current = self.free.load(Ordering::Acquire);
            if current.is_null() {
                break;
            }
            // SAFETY: pooled nodes are freed only in Drop.
            let next = unsafe { (*current).next.load(Ordering::Acquire) };
            if self
                .free
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                unsafe { (*current).value.store(value, Ordering::Relaxed) };
                return current;
            }
        }

        let node = Box::into_raw(Box::new(PlainNode {
            value: AtomicI32::new(value),
            next: AtomicPtr::new(ptr::null_mut()),
            all_next: AtomicPtr::new(ptr::null_mut()),
        }));
        loop {
            let head = self.all.load(Ordering::Relaxed);
            unsafe { (*node).all_next.store(head, Ordering::Relaxed) };
            if self
                .all
                .compare_exchange_weak(head, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return node;
            }
        }
    }

    fn push(&self, value: i32) {
        let node = self.alloc(value);
        loop {
            let current = self.head.load(Ordering::Relaxed);
            unsafe { (*node).next.store(current, Ordering::Relaxed) };
            if self
                .head
                .compare_exchange(current, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }

    fn pop(&self) -> Option<i32> {
        loop {
            let current = self.head.load(Ordering::Acquire);
            if current.is_null() {
                return None;
            }
            // SAFETY: pooled nodes are freed only in Drop.
            let next = unsafe { (*current).next.load(Ordering::Acquire) };
            if self
                .head
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let value = unsafe { (*current).value.load(Ordering::Relaxed) };
                self.recycle(current);
                return Some(value);
            }
        }
    }

    fn recycle(&self, node: *mut PlainNode) {
        loop {
            let current = self.free.load(Ordering::Relaxed);
            unsafe { (*node).next.store(current, Ordering::Relaxed) };
            if self
                .free
                .compare_exchange(current, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }
}

impl Drop for PlainStack {
    fn drop(&mut self) {
        let mut node = *self.all.get_mut();
        while !node.is_null() {
            // SAFETY: `&mut self`, and every node on this list came from
            // Box::into_raw and appears on it once.
            let boxed = unsafe { Box::from_raw(node) };
            node = boxed.all_next.load(Ordering::Relaxed);
        }
    }
}

trait Stack: Send + Sync + 'static {
    fn new() -> Self;
    fn push(&self, value: i32);
    fn pop(&self) -> Option<i32>;
}

impl Stack for LockFreeStack {
    fn new() -> Self {
        LockFreeStack::new()
    }
    fn push(&self, value: i32) {
        LockFreeStack::push(self, value);
    }
    fn pop(&self) -> Option<i32> {
        LockFreeStack::pop(self)
    }
}

impl Stack for PlainStack {
    fn new() -> Self {
        PlainStack::new()
    }
    fn push(&self, value: i32) {
        PlainStack::push(self, value);
    }
    fn pop(&self) -> Option<i32> {
        PlainStack::pop(self)
    }
}

/// Wall time per push and pop pair with `threads` threads running pairs at once.
fn push_pop<S: Stack>(c: &mut Criterion, name: &str) {
    let mut group = c.benchmark_group("push_pop");
    for threads in [1u64, 8] {
        group.bench_with_input(BenchmarkId::new(name, threads), &threads, |b, &threads| {
            b.iter_custom(|iters| {
                let stack = Arc::new(S::new());
                let per_thread = iters.div_ceil(threads);
                let start = Arc::new(Barrier::new(threads as usize + 1));
                let handles: Vec<_> = (0..threads)
                    .map(|_| {
                        let stack = Arc::clone(&stack);
                        let start = Arc::clone(&start);
                        thread::spawn(move || {
                            start.wait();
                            for _ in 0..per_thread {
                                stack.push(black_box(1));
                                black_box(stack.pop());
                            }
                        })
                    })
                    .collect();
                start.wait();
                let began = Instant::now();
                for handle in handles {
                    handle.join().unwrap();
                }
                let elapsed = began.elapsed();
                // Scaled to `iters` so it reads as time per pair across all threads.
                let pairs = u128::from(per_thread * threads);
                Duration::from_nanos((elapsed.as_nanos() * u128::from(iters) / pairs) as u64)
            });
        });
    }
    group.finish();
}

fn bench_head_width(c: &mut Criterion) {
    push_pop::<LockFreeStack>(c, "tagged_128");
    push_pop::<PlainStack>(c, "plain_64");
}

criterion_group!(benches, bench_head_width);
criterion_main!(benches);
