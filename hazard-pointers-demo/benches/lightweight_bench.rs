use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use hazard_pointers_demo::LockFreeStack;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn custom_criterion() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(1))
        .warm_up_time(Duration::from_secs(1))
}

fn lightweight_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("LockFreeStack Operations");

    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_secs(1));
    group.sample_size(10);

    // Single-threaded push
    group.bench_function("push", |b| {
        b.iter_batched(
            || LockFreeStack::<i32>::new(false),
            |stack| {
                stack.push(42).expect("Push should succeed");
            },
            BatchSize::SmallInput,
        );
    });

    // Single-threaded pop
    group.bench_function("pop", |b| {
        b.iter_batched(
            || {
                let stack = LockFreeStack::new(false);
                stack.push(42).expect("Push should succeed");
                stack
            },
            |stack| {
                let _ = stack.pop();
            },
            BatchSize::SmallInput,
        );
    });

    // Very limited concurrent operations
    group.bench_function("concurrent_ops_2_threads", |b| {
        b.iter_batched(
            || Arc::new(LockFreeStack::<i32>::new(false)),
            |stack| {
                let stack2 = Arc::clone(&stack);

                let handle1 = thread::spawn(move || {
                    stack.push(1).expect("Push should succeed");
                    stack.push(2).expect("Push should succeed");
                });

                let handle2 = thread::spawn(move || {
                    let _ = stack2.pop();
                    let _ = stack2.pop();
                });

                handle1.join().expect("Thread 1 panicked");
                handle2.join().expect("Thread 2 panicked");
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = custom_criterion();
    targets = lightweight_bench
}
criterion_main!(benches);
