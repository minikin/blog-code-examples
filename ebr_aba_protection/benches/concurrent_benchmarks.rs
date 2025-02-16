use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ebr_aba_protection::{LockFreeQueue, LockFreeStack};
use std::sync::{Arc, Mutex};
use std::thread;

// Traditional mutex-based stack for comparison
struct MutexStack<T> {
    inner: Mutex<Vec<T>>,
}

impl<T> MutexStack<T> {
    fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    fn push(&self, value: T) {
        self.inner.lock().unwrap().push(value);
    }

    fn pop(&self) -> Option<T> {
        self.inner.lock().unwrap().pop()
    }
}

fn bench_single_threaded(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_threaded");

    group.bench_function("ebr_stack_push", |b| {
        let stack = LockFreeStack::new();
        b.iter(|| stack.push(1));
    });

    group.bench_function("mutex_stack_push", |b| {
        let stack = MutexStack::new();
        b.iter(|| stack.push(1));
    });

    group.bench_function("ebr_queue_enqueue", |b| {
        let queue = LockFreeQueue::new();
        b.iter(|| queue.enqueue(1));
    });

    group.finish();
}

fn bench_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent");

    for threads in [2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("ebr_stack_mixed", threads),
            threads,
            |b, &threads| {
                let stack = Arc::new(LockFreeStack::new());
                b.iter(|| {
                    let handles: Vec<_> = (0..threads)
                        .map(|_| {
                            let stack = Arc::clone(&stack);
                            thread::spawn(move || {
                                for _ in 0..100 {
                                    if rand::random::<bool>() {
                                        let _ = stack.push(1);
                                    } else {
                                        let _ = stack.pop();
                                    }
                                }
                            })
                        })
                        .collect();
                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("mutex_stack_mixed", threads),
            threads,
            |b, &threads| {
                let stack = Arc::new(MutexStack::new());
                b.iter(|| {
                    let handles: Vec<_> = (0..threads)
                        .map(|_| {
                            let stack = Arc::clone(&stack);
                            thread::spawn(move || {
                                for _ in 0..100 {
                                    if rand::random::<bool>() {
                                        stack.push(1);
                                    } else {
                                        let _ = stack.pop();
                                    }
                                }
                            })
                        })
                        .collect();
                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_single_threaded, bench_concurrent);
criterion_main!(benches);
