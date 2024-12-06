# Invocation counter

[![Rust](https://github.com/oramasearch/invocation-counter/actions/workflows/ci.yml/badge.svg)](https://github.com/oramasearch/invocation-counter/actions/workflows/ci.yml)

This data structure responses the following question: how many times a function has been called in the last X minutes?

## Installation

```text
cargo add invocation-counter
```

## Example

```rust
use invocation_counter::Counter;

fn main() {
    // 4 is the group_shift_factor
    // 16 is the number of buckets
    let counter = Counter::<16, 4>::new(4);

    let mut now = 0; // Instant::now().elapsed().as_secs();
    counter.increment_by_one(now);

    now += 1; // Simulate a second passing
    counter.increment_by_one(now);

    assert_eq!(counter.get_count_till(now), 2);

    now += 2_u64.pow(4); // Simulate 16 seconds passing
    counter.increment_by_one(now);

    now += 1; // Simulate a second passing
    counter.increment_by_one(now);

    assert_eq!(counter.get_count_till(now), 4);

    now += 2_u64.pow(4) * 16; // Move foward for a while...
    counter.increment_by_one(now);
    assert_eq!(counter.get_count_till(now), 1); // The counter should have reset
}
```
