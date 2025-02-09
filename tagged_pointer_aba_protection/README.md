# Lock-Free Stack with ABA Protection

This repository contains the code example for the blog post about implementing a lock-free stack in Rust with ABA protection using tagged pointers.

## The ABA Problem

The ABA problem is a common issue in lock-free algorithms where a thread incorrectly assumes that if a value hasn't changed, the data structure hasn't been modified. However, the value might have been changed to something else and then changed back, making this assumption invalid.

```
Time →
Thread 1: Read A         →→→→→→→→ Try CAS(A->C) - Problem!
Thread 2:    Remove A → Add B → Remove B → Add A
```

## Solution: Version Counting

This implementation uses version counting to prevent the ABA problem. Each pointer is paired with a version number that gets incremented on every modification, even if the same value is being added back to the stack.

## Running the Tests

To run the regular test suite:
```bash
cargo test
```

To run the benchmarks (requires nightly Rust):
```bash
cargo +nightly bench
```

## Implementation Details

The implementation uses:
- Atomic operations with `AtomicU128`
- Version counting for ABA prevention
- Safe memory management with Rust's ownership system
- Full test coverage including concurrent operation tests

## Safety Notes

This implementation:
- Is lock-free (progress guarantee)
- Is memory safe (leverages Rust's ownership system)
- Handles concurrent push/pop operations
- Prevents the ABA problem
- Uses atomic operations for thread safety

## License

MIT License - feel free to use this code in your own projects.