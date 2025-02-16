# Lock-Free Data Structures with EBR-based ABA Protection

This repository contains the code examples for implementing lock-free data structures in Rust with ABA protection using Epoch-Based Reclamation (EBR).

## The ABA Problem and EBR Solution

The ABA problem occurs in lock-free algorithms when a thread incorrectly assumes that if a value hasn't changed, the data structure hasn't been modified. EBR solves this by:

1. Managing memory safely through epochs
2. Preventing premature memory reclamation
3. Ensuring nodes aren't reused while other threads might still access them

```
Time →
Thread 1: Read A         →→→→→→→→ Safe operation! (EBR prevents reuse)
Thread 2:    Remove A → Add B → Remove B → Add new_A
```

## Implementation Details

The implementation uses:
- Crossbeam's epoch-based memory reclamation (`crossbeam-epoch`)
- Lock-free algorithms for stack and queue implementations
- Safe concurrent memory management
- Comprehensive benchmarking suite

## Running the Tests and Benchmarks

To run the regular test suite:
```bash
cargo test
```

To run the benchmarks:
```bash
cargo bench
```

The benchmarks compare:
- EBR-protected stack operations
- EBR-protected queue operations
- Mutex-based implementations (as baseline)
- Single-threaded vs concurrent performance

## Data Structures

The repository includes:
- Lock-free Stack implementation
- Lock-free Queue implementation
- Benchmark comparisons with traditional mutex-based approaches

## Safety Features

This implementation:
- Is lock-free (progress guarantee)
- Is memory safe through EBR
- Handles concurrent operations safely
- Prevents the ABA problem without atomic version counting
- Uses Rust's type system for compile-time guarantees

## Dependencies

- `crossbeam-epoch`: For epoch-based reclamation
- `crossbeam-utils`: For additional concurrent utilities
- `rand`: For testing and benchmarking
- `criterion`: For benchmarking

## License

MIT License - feel free to use this code in your own projects.