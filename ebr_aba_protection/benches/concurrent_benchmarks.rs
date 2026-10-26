//! The workloads behind the table in the post: one thread doing a push and a
//! pop, and 2, 4 and 8 threads pushing concurrently. `MutexLinked` allocates a
//! node per push like the EBR stack does, so it isolates the cost of locking
//! from the cost of a linked layout.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ebr_aba_protection::{LockFreeQueue, LockFreeStack};
use std::hint::black_box;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

trait Stack: Send + Sync + 'static {
    fn new() -> Self;
    fn push(&self, value: u64);
    fn pop(&self) -> Option<u64>;
}

impl Stack for LockFreeStack<u64> {
    fn new() -> Self {
        LockFreeStack::new()
    }
    fn push(&self, value: u64) {
        LockFreeStack::push(self, value).unwrap();
    }
    fn pop(&self) -> Option<u64> {
        LockFreeStack::pop(self)
    }
}

struct MutexVec(Mutex<Vec<u64>>);

impl Stack for MutexVec {
    fn new() -> Self {
        MutexVec(Mutex::new(Vec::new()))
    }
    fn push(&self, value: u64) {
        self.0.lock().unwrap().push(value);
    }
    fn pop(&self) -> Option<u64> {
        self.0.lock().unwrap().pop()
    }
}

struct LinkedNode {
    value: u64,
    next: Option<Box<LinkedNode>>,
}

struct MutexLinked(Mutex<Option<Box<LinkedNode>>>);

impl Stack for MutexLinked {
    fn new() -> Self {
        MutexLinked(Mutex::new(None))
    }
    fn push(&self, value: u64) {
        let mut head = self.0.lock().unwrap();
        let next = head.take();
        *head = Some(Box::new(LinkedNode { value, next }));
    }
    fn pop(&self) -> Option<u64> {
        let mut head = self.0.lock().unwrap();
        let node = head.take()?;
        *head = node.next;
        Some(node.value)
    }
}

impl Drop for MutexLinked {
    fn drop(&mut self) {
        // Iterative, so a long list does not overflow the stack through
        // recursive `Box` drops.
        let mut next = self.0.get_mut().unwrap().take();
        while let Some(node) = next {
            next = node.next;
        }
    }
}

fn push_pop<S: Stack>(c: &mut Criterion, name: &str) {
    c.bench_function(&format!("single_threaded/push_pop/{name}"), |b| {
        let stack = S::new();
        b.iter(|| {
            stack.push(black_box(1));
            black_box(stack.pop())
        });
    });
}

/// Time per push with `threads` threads pushing into one stack at once.
fn concurrent_push<S: Stack>(c: &mut Criterion, name: &str) {
    let mut group = c.benchmark_group("concurrent_push");
    for threads in [2u64, 4, 8] {
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
                            for i in 0..per_thread {
                                stack.push(black_box(i));
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
                // Wall time over all pushes, scaled to `iters` so it reads as
                // time per push.
                let pushes = u128::from(per_thread * threads);
                Duration::from_nanos((elapsed.as_nanos() * u128::from(iters) / pushes) as u64)
            });
        });
    }
    group.finish();
}

fn bench_single_threaded(c: &mut Criterion) {
    push_pop::<LockFreeStack<u64>>(c, "ebr_stack");
    push_pop::<MutexVec>(c, "mutex_vec");
    push_pop::<MutexLinked>(c, "mutex_linked");

    c.bench_function("single_threaded/enqueue_dequeue/ebr_queue", |b| {
        let queue = LockFreeQueue::new();
        b.iter(|| {
            queue.enqueue(black_box(1u64));
            black_box(queue.dequeue())
        });
    });
}

fn bench_concurrent(c: &mut Criterion) {
    concurrent_push::<LockFreeStack<u64>>(c, "ebr_stack");
    concurrent_push::<MutexVec>(c, "mutex_vec");
    concurrent_push::<MutexLinked>(c, "mutex_linked");
}

criterion_group!(benches, bench_single_threaded, bench_concurrent);
criterion_main!(benches);
