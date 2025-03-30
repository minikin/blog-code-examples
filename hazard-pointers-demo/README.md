# Hazard Pointers Demo

This is a demonstration of using hazard pointers to solve the ABA problem in lock-free data structures, as described in the blog post [Solving the ABA Problem in Rust with Hazard Pointers](https://minikin.me/blog/solving-the-aba-problem-in-rust-hazard-pointers).

## What is the ABA Problem?

The ABA problem occurs in concurrent programming when a thread reads a value A from a shared memory location, gets preempted, another thread changes the value to B and then back to A, and the first thread resumes, incorrectly assuming that the value has not changed since it was last read.

Hazard pointers solve this by providing a way to protect specific memory locations from being recycled while they're in use.

## Running the Demo

```bash
# Run the basic demo that shows protection against ABA
cargo run

# Run the benchmarks
cargo bench
```

## Implementation Details

This demo includes:

1. A complete lock-free stack implementation using hazard pointers
2. An ABA problem demonstration showing how hazard pointers protect against it
3. Comparison with other techniques (comments in the code)
4. Performance benchmarks (run with `cargo bench`)

## Learning More

For more information about hazard pointers and other solutions to the ABA problem, check out the following blog posts:

- [Part 1: Tagged Pointers with Versioning](https://minikin.me/blog/solving-the-aba-problem-in-rust-tagged-pointers)
- [Part 2: Epoch-Based Reclamation](https://minikin.me/blog/epoch-adventures-breaking-free-from-aba-in-concurrent-rust)
- [Part 3: Hazard Pointers](https://minikin.me/blog/solving-the-aba-problem-in-rust-hazard-pointers) 