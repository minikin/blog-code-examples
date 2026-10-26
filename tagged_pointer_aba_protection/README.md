# Lock-free stack with tagged pointers

Code for [Solving the ABA Problem in Rust with Tagged Pointers](https://minikin.me/blog/solving-the-aba-problem-in-rust-tagged-pointers/), part 1 of the ABA series.

The stack head is a pointer and a 64-bit version packed into one `u128`. Every successful CAS bumps the version, so a thread holding a stale head loses its CAS even when the address has come back.

The tag does not make it safe to read a node another thread has popped. Popped nodes go on a second tagged free list and are reused, and the pool is freed only in `Drop`, so a stale pointer always points at live memory. `next` is an `AtomicPtr` because a thread holding a stale tag reads it while the node's current owner writes it. Pointers are packed with `expose_provenance` and unpacked with `with_exposed_provenance_mut`, since `addr()` drops the provenance the round trip needs.

`AtomicU128` comes from `portable-atomic`, so the crate builds on stable.

## Running

```bash
cargo run
cargo test
```

`cargo run` forces one interleaving with two channels and prints the same output every time:

```text
T1 read the head at version 3
T2 popped 3
T2 popped 2
T2 pushed 3 back, head is at version 6
T1 CAS failed: it expected version 3, found 6
stack from the top: 3 1
```

Miri checks the unsafe code, including the threaded tests. Packing a pointer into an integer is exposed provenance, so it needs `-Zmiri-permissive-provenance`:

```bash
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-permissive-provenance" cargo +nightly miri test
```

`cargo bench` runs the comparison behind the table in the post: this stack against the same pooled stack with plain `AtomicPtr` heads, one thread and eight, each doing a push and then a pop. The plain version is ABA-prone and exists only to price the 64-bit CAS against the 128-bit one.

## License

MIT
