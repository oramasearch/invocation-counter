# invocation-counter

[![Rust](https://github.com/oramasearch/invocation-counter/actions/workflows/ci.yml/badge.svg)](https://github.com/oramasearch/invocation-counter/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/invocation-counter.svg)](https://crates.io/crates/invocation-counter)
[![Documentation](https://docs.rs/invocation-counter/badge.svg)](https://docs.rs/invocation-counter)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

A high-performance, thread-safe data structure for tracking function invocation counts over sliding time windows. Perfect for rate limiting, monitoring, and analytics use cases.

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
invocation-counter = "*"
```

## Basic Usage

```rust
use invocation_counter::InvocationCounter;

// Create a counter with 8 slots × 16 time units = 128-unit sliding window
let counter = InvocationCounter::new(3, 4);  // 2^3 slots, 2^4 time units per slot

// Register function calls
counter.register(10);   // Called at time 10
counter.register(25);   // Called at time 25
counter.register(50);   // Called at time 50

// Query how many calls in the last 128 time units
assert_eq!(counter.count_in(0, 100), 3); // All calls within range
```

## Core Concepts

### Time Units
Time units are abstract - they can represent milliseconds, seconds, minutes, or any consistent measurement. The counter works with `u64` timestamps where larger values represent later points in time.

### Sliding Windows
The counter tracks invocations within a sliding time window that moves forward with each query. The window size is determined by:

**Window Size = 2^slot_count_exp × 2^slot_size_exp**

### Configuration Parameters

- **`slot_count_exp`**: Exponent for number of slots (2^slot_count_exp total slots)
- **`slot_size_exp`**: Exponent for time units per slot (2^slot_size_exp time units per slot)

**Trade-offs:**
- More slots = better precision, more memory usage
- Larger slot size = longer windows, less precision
- The counter provides approximate counts optimized for performance

## Important Notes

### Approximation
The counter provides **approximate** counts due to its interval-based design:
- Invocations are grouped into discrete time intervals
- Recent invocations may be counted even if slightly outside the exact window
- This approximation enables high performance and fixed memory usage

## License

Licensed under the Apache License, Version 2.0. See LICENSE file for details.