# Hazard pointers demo

Code for [Solving the ABA Problem in Rust with Hazard Pointers](https://minikin.me/blog/solving-the-aba-problem-in-rust-hazard-pointers/), part 3 of the ABA series.

`pop` publishes the head in the calling thread's hazard slot, re-reads `head` to validate the publication, and retires the node after winning the CAS. A retired node is freed only by a scan that finds it in no slot.

- The registry is protected by a mutex. That keeps the ordering argument short, and it means `LockFreeStack` here is not actually lock-free. `HazardSlot` in `src/slot.rs` shows the lock-free publication with `SeqCst` on the store and the re-read.
- `protect` returns a `HazardGuard` that clears the slot on every exit path, including early returns and panics.
- `retire` releases the retire list lock before it calls `try_reclaim`, since `std::sync::Mutex` is not reentrant.
- `Node::value` is a `ManuallyDrop<T>`: `pop` moves the value out, and reclaiming the node later frees the allocation without dropping the value again.
- The registry is `Send` and `Sync` only for `T: Send`.
- `src/buffer_pool.rs` is the buffer pool sketch from the post.

## Running

```bash
cargo run                                  # the ABA walkthrough
cargo run -- --stress-test --no-show-aba-demo
cargo test
cargo bench
```

The walkthrough forces one interleaving with two channels: thread 1 publishes the head, thread 2 pops two nodes and forces a scan, which frees the unpublished node and keeps the published one, and thread 1's re-read of `head` then fails.

Run the tests under Miri as well:

```bash
MIRIFLAGS="-Zmiri-strict-provenance" cargo +nightly miri test
```

## Further reading

- [Part 1: Tagged Pointers with Versioning](https://minikin.me/blog/solving-the-aba-problem-in-rust-tagged-pointers/)
- [Part 2: Epoch-Based Reclamation](https://minikin.me/blog/epoch-adventures-breaking-free-from-aba-in-concurrent-rust/)
