#![doc = include_str!("../README.md")]

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Debug)]
struct Slot {
    interval_start: AtomicU64,
    counter: AtomicU32,
}

impl Slot {
    fn new() -> Self {
        Self {
            interval_start: AtomicU64::new(0),
            counter: AtomicU32::new(0),
        }
    }
}

/// A structure for tracking invocation counts over sliding time windows.
///
/// `InvocationCounter` implements a ring buffer-based algorithm that efficiently answers the question:
/// "How many times has a function been called in the last X time units?" The structure divides time
/// into fixed-size intervals and uses a circular array of slots to track counts within each interval.
///
/// # Time Units
///
/// Time units are abstract and can represent any consistent time measurement (milliseconds,
/// seconds, ticks, etc.). The counter treats time as unsigned 64-bit integers, where larger
/// values represent later points in time.
///
/// # Count Approximation
///
/// The counter provides an **approximate** count due to its interval-based design:
/// - Invocations are grouped into discrete time intervals (slots)
/// - The sliding window moves in interval-sized steps, not continuously
/// - Recent invocations may be counted even if they fall slightly outside the exact window
/// - Very old invocations may be excluded if their slots have been reused
///
/// This approximation trades perfect accuracy for significant performance and memory efficiency.
///
/// # Architecture
///
/// The counter uses a ring buffer with 2^`slot_count_exp` slots, where each slot represents a time
/// interval of 2^`slot_size_exp` time units. The total sliding window size is therefore:
/// `2^slot_count_exp × 2^slot_size_exp` time units.
///
/// Each slot contains:
/// - `interval_start`: The start timestamp of the time interval this slot represents
/// - `counter`: The number of invocations registered in this interval
///
/// As time progresses, slots are reused in a circular fashion. When a new time interval begins
/// that maps to an already-occupied slot, the slot is reset and begins tracking the new interval.
///
/// # Example
///
/// ```rust
/// # use invocation_counter::InvocationCounter;
///
/// // Create counter: 8 slots × 16 time units = 128 time unit sliding window
/// let counter = InvocationCounter::new(3, 4);  // 2^3 slots, 2^4 time units per slot
///
/// counter.register(10);  // Register invocation at time 10
/// counter.register(25);  // Register invocation at time 25
///
/// assert_eq!(counter.count_in(0, 26), 2);     // both in range [0, 26)
/// assert_eq!(counter.count_in(0, 16), 1);     // only first (slot 0: times 0-15)
/// assert_eq!(counter.count_in(16, 32), 1);    // only second (slot 1: times 16-31)
/// assert_eq!(counter.count_in(50, 60), 0);    // out of range
///
/// counter.register(200);
/// assert_eq!(counter.count_in(200 - 128, 201), 1);  // Only the last in 128-unit window
/// ```
#[derive(Debug)]
pub struct InvocationCounter {
    slots: Box<[Slot]>,
    slot_count_exp: u8,
    slot_size_exp: u8,
    max_current_time: AtomicU64,
}

impl InvocationCounter {
    /// Creates a new `InvocationCounter` with the specified configuration.
    ///
    /// # Arguments
    ///
    /// * `slot_count_exp` - Exponent for the number of slots (2^slot_count_exp slots total)
    /// * `slot_size_exp` - Exponent for the size of each time interval (2^slot_size_exp time units per slot)
    ///
    /// The total sliding window size will be: `2^slot_count_exp × 2^slot_size_exp` time units.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use invocation_counter::InvocationCounter;
    /// // Create a counter with 8 slots (2^3), each covering 16 time units (2^4)
    /// // Total window: 8 × 16 = 128 time units
    /// let counter = InvocationCounter::new(3, 4);
    /// ```
    pub fn new(slot_count_exp: u8, slot_size_exp: u8) -> Self {
        let slots = (0..(1 << slot_count_exp))
            .map(|_| Slot::new())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            slots,
            slot_count_exp,
            slot_size_exp,
            max_current_time: AtomicU64::new(0),
        }
    }

    /// Registers an invocation at the specified time.
    ///
    /// This method is thread-safe. Multiple threads can call this method
    /// concurrently without external synchronization.
    ///
    /// # Arguments
    ///
    /// * `current_time` - The timestamp when the invocation occurred
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use invocation_counter::InvocationCounter;
    /// let counter = InvocationCounter::new(3, 4); // 8 slots × 16 units = 128-unit window
    ///
    /// counter.register(10);
    /// counter.register(10); // Same interval, increments counter
    /// counter.register(25); // Different interval, uses different slot
    /// ```
    pub fn register(&self, current_time: u64) {
        let interval_start = current_time >> self.slot_size_exp;

        let slot_index = interval_start % (1 << self.slot_count_exp);

        let interval_start = interval_start << self.slot_size_exp;

        let slot = &self.slots[slot_index as usize];

        let time_in_slot = slot.interval_start.load(Ordering::Acquire);
        if time_in_slot == interval_start {
            slot.counter.fetch_add(1, Ordering::Relaxed);
        } else {
            slot.interval_start.store(interval_start, Ordering::Release);
            slot.counter.store(1, Ordering::Release);
        }

        let current_max_time = self.max_current_time.load(Ordering::Acquire);
        if current_max_time < current_time {
            self.max_current_time
                .compare_exchange_weak(
                    current_max_time,
                    current_time,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .ok();
        }
    }

    /// Returns the total number of invocations within the specified time range.
    ///
    /// Unlike `count()` which uses a fixed sliding window, this method allows querying
    /// invocations within any arbitrary time range defined by `start_time` and `end_time`.
    /// The method still respects the ring buffer's current valid data range.
    ///
    /// # Arguments
    ///
    /// * `start_time` - The start time of the range to query (inclusive)
    /// * `end_time` - The end time of the range to query (exclusive)
    ///
    /// # Returns
    ///
    /// The total number of invocations that occurred within the specified time range,
    /// limited by the data currently available in the ring buffer.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use invocation_counter::InvocationCounter;
    /// let counter = InvocationCounter::new(3, 4); // 8 slots × 16 units = 128-unit window
    ///
    /// counter.register(10);
    /// counter.register(25);
    /// counter.register(100);
    ///
    /// assert_eq!(counter.count_in(0, 50), 2);   // First two invocations
    /// assert_eq!(counter.count_in(20, 120), 2); // Second and third invocations
    /// assert_eq!(counter.count_in(0, 200), 3);  // All invocations (if within ring buffer range)
    /// ```
    pub fn count_in(&self, start_time: u64, end_time: u64) -> u32 {
        if start_time >= end_time {
            return 0;
        }

        let current_max_time = self.max_current_time.load(Ordering::Acquire);

        // Calculate the ring buffer's valid range (same as count method)
        let ring_end = ((current_max_time >> self.slot_size_exp) + 1) << self.slot_size_exp;
        let ring_start =
            ring_end.saturating_sub((1 << self.slot_size_exp) * (1 << self.slot_count_exp));
        let ring_buffer_range = ring_start..ring_end;

        // Calculate the requested range, aligning to slot boundaries
        // start_time is inclusive: include the slot that contains start_time
        let asked_start = start_time >> self.slot_size_exp << self.slot_size_exp;
        // end_time is exclusive: find the slot that would contain end_time and use its start as boundary
        // If end_time is exactly at a slot boundary, use that boundary
        // Otherwise, use the start of the next slot after the slot containing end_time
        let asked_end = if end_time & ((1 << self.slot_size_exp) - 1) == 0 {
            // end_time is exactly at slot boundary
            end_time
        } else {
            // end_time is within a slot, use start of next slot
            ((end_time >> self.slot_size_exp) + 1) << self.slot_size_exp
        };
        let asked_range = asked_start..asked_end;

        // Find the intersection of ring buffer range and requested range
        let valid_range = ring_buffer_range.start.max(asked_range.start)
            ..ring_buffer_range.end.min(asked_range.end);

        let mut count = 0;
        for slot in &self.slots {
            let time_in_slot = slot.interval_start.load(Ordering::Acquire);
            if valid_range.contains(&time_in_slot) {
                count += slot.counter.load(Ordering::Acquire);
            }
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_basic_functionality() {
        // 4 slots (2^2) * 8 time units (2^3) = 32 time units window
        let counter = InvocationCounter::new(2, 3);

        counter.register(0);
        counter.register(1);
        counter.register(8);
        counter.register(16);

        // All within 32-unit window from time 16
        assert_eq!(counter.count_in(0, 16 + 1), 4);

        // All outside window from time 100
        assert_eq!(counter.count_in(100 - 32, 100 + 1), 0);
    }

    #[test]
    fn test_count_in() {
        // 2 slots (2^1) * 4 time units (2^2) = 8 time units window
        // Slot 0: times 0-3 (interval_start = 0)
        // Slot 1: times 4-7 (interval_start = 4)
        let counter = InvocationCounter::new(1, 2);

        counter.register(0);

        // All registrations 0,1,2,3 go to the same slot (interval_start = 0)
        // count_in works on slot boundaries, so any range that includes slot 0 gets all counts
        assert_eq!(counter.count_in(0, 1), 1); // Includes slot 0
        assert_eq!(counter.count_in(0, 4), 1); // Includes slot 0
        assert_eq!(counter.count_in(0, 8), 1); // Includes slot 0
        assert_eq!(counter.count_in(4, 8), 0); // Only includes slot 1 (empty)

        counter.register(1);
        // Still all in slot 0
        assert_eq!(counter.count_in(0, 4), 2); // Includes slot 0
        assert_eq!(counter.count_in(0, 8), 2); // Includes slot 0
        assert_eq!(counter.count_in(4, 8), 0); // Only slot 1

        counter.register(2);
        assert_eq!(counter.count_in(0, 4), 3); // Slot 0
        assert_eq!(counter.count_in(4, 8), 0); // Slot 1

        counter.register(3);
        assert_eq!(counter.count_in(0, 4), 4); // Slot 0
        assert_eq!(counter.count_in(4, 8), 0); // Slot 1

        // Register 4 in slot 1 (interval_start = 4)
        counter.register(4);
        assert_eq!(counter.count_in(0, 4), 4); // Slot 0 only (end_time=4 excludes slot 1)
        assert_eq!(counter.count_in(4, 8), 1); // Slot 1 only
        assert_eq!(counter.count_in(0, 8), 5); // Both slots

        counter.register(5);
        assert_eq!(counter.count_in(0, 4), 4); // Slot 0 only
        assert_eq!(counter.count_in(4, 8), 2); // Slot 1 only
        assert_eq!(counter.count_in(0, 8), 6); // Both slots

        counter.register(6);
        assert_eq!(counter.count_in(0, 4), 4); // Slot 0 only
        assert_eq!(counter.count_in(4, 8), 3); // Slot 1 only
        assert_eq!(counter.count_in(0, 8), 7); // Both slots

        counter.register(7);
        assert_eq!(counter.count_in(0, 4), 4); // Slot 0 only
        assert_eq!(counter.count_in(4, 8), 4); // Slot 1 only
        assert_eq!(counter.count_in(0, 8), 8); // Both slots
        assert_eq!(counter.count_in(4, 12), 4); // Slot 1 only
        assert_eq!(counter.count_in(12, 16), 0); // Out of range

        // Register 8 - wraps to slot 0, resets it (interval_start = 8)
        counter.register(8);
        assert_eq!(counter.count_in(0, 4), 0); // Old slot 0 data gone
        assert_eq!(counter.count_in(4, 8), 4); // Slot 1 unchanged
        assert_eq!(counter.count_in(8, 12), 1); // New slot 0
        assert_eq!(counter.count_in(4, 12), 5); // Slot 1 + new slot 0
        assert_eq!(counter.count_in(8, 9), 1); // Within new slot 0
        assert_eq!(counter.count_in(0, 12), 5); // Full range

        counter.register(9);
        assert_eq!(counter.count_in(0, 4), 0); // Old slot 0 still gone
        assert_eq!(counter.count_in(4, 8), 4); // Slot 1 unchanged
        assert_eq!(counter.count_in(8, 12), 2); // New slot 0 has 2 entries
        assert_eq!(counter.count_in(4, 12), 6); // Slot 1 + new slot 0
        assert_eq!(counter.count_in(8, 10), 2); // Within new slot 0
        assert_eq!(counter.count_in(0, 16), 6); // Full range

        counter.register(10);
        assert_eq!(counter.count_in(4, 8), 4); // Slot 1 unchanged
        assert_eq!(counter.count_in(8, 12), 3); // New slot 0 has 3 entries
        assert_eq!(counter.count_in(4, 12), 7); // Slot 1 + new slot 0
        assert_eq!(counter.count_in(8, 11), 3); // Within new slot 0
        assert_eq!(counter.count_in(0, 16), 7); // Full range

        counter.register(11);
        assert_eq!(counter.count_in(4, 8), 4); // Slot 1 unchanged
        assert_eq!(counter.count_in(8, 12), 4); // New slot 0 has 4 entries
        assert_eq!(counter.count_in(4, 12), 8); // Slot 1 + new slot 0
        assert_eq!(counter.count_in(8, 12), 4); // Full new slot 0
        assert_eq!(counter.count_in(10, 12), 4); // Includes full slot 0 (slot-boundary aligned)
        assert_eq!(counter.count_in(0, 16), 8); // Full range

        // Register 12 - wraps to slot 1, resets it (interval_start = 12)
        counter.register(12);
        assert_eq!(counter.count_in(4, 8), 0); // Old slot 1 data gone
        assert_eq!(counter.count_in(8, 12), 4); // Slot 0 unchanged
        assert_eq!(counter.count_in(12, 16), 1); // New slot 1
        assert_eq!(counter.count_in(8, 16), 5); // Slot 0 + new slot 1
        assert_eq!(counter.count_in(0, 16), 5); // Full range
        assert_eq!(counter.count_in(12, 13), 1); // Within new slot 1

        counter.register(13);
        assert_eq!(counter.count_in(8, 12), 4); // Slot 0 unchanged
        assert_eq!(counter.count_in(12, 16), 2); // New slot 1 has 2 entries
        assert_eq!(counter.count_in(8, 16), 6); // Slot 0 + new slot 1
        assert_eq!(counter.count_in(0, 16), 6); // Full range
        assert_eq!(counter.count_in(12, 14), 2); // Within new slot 1

        counter.register(14);
        assert_eq!(counter.count_in(8, 12), 4); // Slot 0 unchanged
        assert_eq!(counter.count_in(12, 16), 3); // New slot 1 has 3 entries
        assert_eq!(counter.count_in(8, 16), 7); // Slot 0 + new slot 1
        assert_eq!(counter.count_in(0, 16), 7); // Full range
        assert_eq!(counter.count_in(12, 15), 3); // Within new slot 1

        counter.register(15);
        assert_eq!(counter.count_in(8, 12), 4); // Slot 0 unchanged
        assert_eq!(counter.count_in(12, 16), 4); // New slot 1 has 4 entries
        assert_eq!(counter.count_in(8, 16), 8); // Slot 0 + new slot 1
        assert_eq!(counter.count_in(0, 16), 8); // Full range
        assert_eq!(counter.count_in(12, 16), 4); // Full new slot 1

        // Register 16 - wraps to slot 0 again, resets it (interval_start = 16)
        counter.register(16);
        assert_eq!(counter.count_in(8, 12), 0); // Old slot 0 data gone
        assert_eq!(counter.count_in(12, 16), 4); // Slot 1 unchanged
        assert_eq!(counter.count_in(16, 20), 1); // New slot 0
        assert_eq!(counter.count_in(12, 20), 5); // Slot 1 + new slot 0
        assert_eq!(counter.count_in(0, 20), 5); // Full range
        assert_eq!(counter.count_in(16, 17), 1); // Within new slot 0
    }

    #[test]
    fn test_slot_reuse() {
        // 4 slots (2^2) * 4 time units (2^2) = 16 time units window
        let counter = InvocationCounter::new(2, 2);

        counter.register(0); // slot 0
        counter.register(16); // wraps to slot 0, resets counter

        assert_eq!(counter.count_in(0, 17), 1); // Only recent registration
    }

    #[test]
    fn test_concurrent_access() {
        let num_threads = 4;
        let registrations_per_thread = 100;

        // 8 slots (2^3) * 64 time units (2^6) = 512 time units window
        let counter = Arc::new(InvocationCounter::new(3, 6));

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let counter_clone = Arc::clone(&counter);
                thread::spawn(move || {
                    for i in 0..registrations_per_thread {
                        counter_clone.register(thread_id as u64 * 10 + i as u64);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let count = counter.count_in(0, 399); // Query at end of all registrations
        assert!(count > 0);
        assert!(count <= num_threads * registrations_per_thread);
    }

    #[test]
    fn test_edge_cases() {
        // 4 slots (2^2) * 8 time units (2^3) = 32 time units window
        let counter = InvocationCounter::new(2, 3);

        // Empty counter
        assert_eq!(counter.count_in(0, 0), 0);

        // Zero time
        counter.register(0);
        assert_eq!(counter.count_in(0, 1), 1);

        // Large time values
        let large_time = 1_000_000u64;
        counter.register(large_time);
        assert_eq!(counter.count_in(large_time, large_time + 1), 1);
    }
}
