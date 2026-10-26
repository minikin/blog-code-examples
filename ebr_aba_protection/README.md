# Lock-free stack and queue with epoch-based reclamation

Code for [Epoch Adventures: Breaking Free from ABA in Concurrent Rust](https://minikin.me/blog/epoch-adventures-breaking-free-from-aba-in-concurrent-rust/), part 2 of the ABA series.

Both structures use `crossbeam-epoch`. A popped node is handed to `defer_destroy` and freed once the global epoch has advanced twice, so its address cannot come back from the allocator while any thread pinned at the time could still hold it.

- `LockFreeStack` is the Treiber stack from the post. The value is a `ManuallyDrop<T>`, so moving it out in `pop` and then destroying the node drops it once. The node stays `Owned` until the CAS takes it, so `push` has no `unsafe` and cannot leak on a retry. There is no retry limit. `with_capacity` claims a slot with a CAS before allocating, so the bound holds under contention.
- `LockFreeQueue` is a Michael-Scott queue with a sentinel node. `dequeue` moves the value out of the node that becomes the new sentinel, values are `MaybeUninit<T>` so no node drops its own value, and `Drop` drops the values still queued.

## Running

```bash
cargo run
cargo test
cargo bench
```

The suite runs under Miri with Tree Borrows. Stacked Borrows reports a violation inside crossbeam's intrusive participant list, and crossbeam's global collector is never freed, hence the leak flag:

```bash
MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks -Zmiri-disable-isolation" cargo +nightly miri test --lib
```

`benches/concurrent_benchmarks.rs` holds the workloads behind the table in the post: one thread doing a push and a pop, and 2, 4 and 8 threads pushing at once, against the EBR stack, a `Mutex<Vec>` and a `Mutex` around a linked list that allocates per push.

## License

MIT
