# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust crate called `invocation-counter` that provides a data structure to answer: "how many times a function has been called in the last X minutes?" It's focused on concurrency, memory management, and efficient algorithms for tracking invocation counts over time windows.

## Development Commands

### Building
- `cargo build` - Build debug version
- `cargo build --release` - Build release version

### Testing  
- `cargo test` - Run all tests

### Code Quality
- `cargo check` - Fast compile check without building
- `cargo fmt` - Format code according to Rust standards
- `cargo clippy` - Run clippy linter for additional checks

### Combined Quality Check
Run all quality checks in sequence (as done in CI):
```bash
cargo check && cargo fmt && cargo clippy
```

## Architecture Notes

### Core Implementation

The main library code is in `src/lib.rs` and implements a lock-free ring buffer design using atomic operations for thread-safe invocation counting.

**Key Components:**

- `InvocationCounter`: The main data structure that tracks invocations over sliding time windows
- `Slot`: Individual ring buffer slots containing atomic counters and interval start times
- Uses `AtomicU32` for counters and `AtomicU64` for timestamps to ensure thread safety

**Algorithm Design:**

- Ring buffer with configurable slot count (2^`slot_count_exp`) and slot size (2^`slot_size_exp`)
- Each slot represents a time interval, with slots reused as time progresses
- Efficient O(slot_count) counting by iterating only through ring buffer slots
- Slots automatically reset when reused for new time intervals

**Public API:**

- `new(slot_count_exp: u8, slot_size_exp: u8)`: Creates counter with 2^slot_count_exp slots, each covering 2^slot_size_exp time units
- `register(current_time: u64)`: Records an invocation at the given timestamp
- `count_in(start_time: u64, end_time: u64)`: Returns total invocations within the specified time range

**Thread Safety:**

The implementation is fully thread-safe using atomic operations with appropriate memory ordering:
- `Acquire`/`Release` ordering for synchronization
- `Relaxed` ordering for performance where strict ordering isn't required
- `compare_exchange_weak` for lock-free max time tracking

**Count Approximation:**

The counter provides approximate counts due to its interval-based design:
- Invocations are grouped into discrete time intervals (slots)
- Time ranges are aligned to slot boundaries for querying
- Recent invocations may be counted even if they fall slightly outside the exact requested range
- This approximation trades perfect accuracy for significant performance and memory efficiency
