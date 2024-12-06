#![doc = include_str!("../README.md")]

use std::{fmt::Debug, sync::Mutex};

#[derive(Default)]
struct Pair {
    key: u64,
    value: usize,
}

/// A counter useful for counting how many times a function invocation is called in last X seconds/minutes/hours.
/// `N` is the number of buckets and `M` is the number of sub-buckets.
///
/// In the documentation and in the code, we use `key` to refer the temporal unit (e.g. seconds, minutes, hours) of the invocation.
/// Because this library don't want force you to use seconds, you can use any unit you want.
/// You can consider to use `std::time::Instant::elapsed().as_secs()` as the key for instance.
///
/// `Counter` groups keys into buckets based on the `group_shift_factor`: `key >> group_shift_factor % N` will be the bucket index.
/// The index for the sub-bucket is `key % M`.
///
/// ## Internal structure
///
/// Internally, the `Counter` uses a ring buffer of `N` buckets. Each bucket has `M` sub-buckets.
/// This allows the `Counter` to distribute the load across multiple sub-buckets when the keys have the same index.
///
/// For instance, `Counter<3, 2>::new(4)` will be like:
/// ```text
///                ----------   ----------   ----------
///                | (0, 0) |   | (0, 0) |   | (0, 0) |
///                | (0, 0) |   | (0, 0) |   | (0, 0) |
///                ----------   ----------   ----------
/// index              0            1            2
/// key range 1       0-16        17-31        32-47
/// key range 2      48-63        64-80         ...
/// ```
///
pub struct Counter<const N: usize, const M: usize> {
    buckets: [[Mutex<Pair>; M]; N],
    group_shift_factor: u32,
}

impl<const N: usize, const M: usize> Debug for Counter<N, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = self
            .buckets
            .iter()
            .map(|b| {
                b.iter()
                    .map(|m| {
                        let pair = m.lock().unwrap();
                        (pair.key, pair.value)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        f.debug_struct("Counter")
            .field("buckets", &data)
            .field("group_shift_factor", &self.group_shift_factor)
            .finish()
    }
}

impl<const N: usize, const M: usize> Counter<N, M> {
    /// Create a new counter with the given group shift factor.
    ///
    /// `group_shift_factor` is the number of bits to shift the key to get the group index.
    pub fn new(group_shift_factor: u32) -> Self {
        let locks = core::array::from_fn(|_| core::array::from_fn(|_| Mutex::new(Pair::default())));

        Self {
            buckets: locks,
            group_shift_factor,
        }
    }

    /// Register an invocation.
    ///
    /// This will increment the value of the key by one.
    /// You can use `std::time::Instant::elapsed().as_secs()` as the key for instance.
    pub fn increment_by_one(&self, key: u64) {
        let index = (key >> self.group_shift_factor) as usize % N;
        let sub_index = (key as usize) % M;

        let mut pair = self.buckets[index][sub_index].lock().unwrap();

        let lower_bound = self.get_lower_bound(key, N);
        if (lower_bound..=key).contains(&pair.key) {
            pair.key = key;
            pair.value += 1;
        } else {
            if pair.key > key {
                return;
            }
            pair.key = key;
            pair.value = 1;
        }
    }

    /// Get the count of invocations till the given key.
    ///
    /// This will return the total count of invocations till the given key.
    /// You can use `std::time::Instant::elapsed().as_secs()` as the key for instance.
    ///
    pub fn get_count_till(&self, key: u64) -> usize {
        let d = 2_u64.pow(self.group_shift_factor) * N as u64;

        let allowed_range = self.get_lower_bound(key, d as usize)..=key;

        let mut tot = 0;
        for b in &self.buckets {
            let mut s = 0;
            for sub in b {
                let pair = sub.lock().unwrap();

                if allowed_range.contains(&pair.key) {
                    s += pair.value;
                }
            }

            tot += s;
        }

        tot
    }

    #[inline]
    fn get_lower_bound(&self, key: u64, n: usize) -> u64 {
        key.saturating_sub(n as u64 - 1)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use crate::Counter;

    #[test]
    fn test_initially_empty() {
        let counter = Counter::<2, 5>::new(4);
        assert_eq!(counter.get_count_till(0), 0);
        assert_eq!(counter.get_count_till(1), 0);
        assert_eq!(counter.get_count_till(2), 0);

        assert_eq!(counter.get_count_till(16), 0);
        assert_eq!(counter.get_count_till(17), 0);

        assert_eq!(counter.get_count_till(16 * 2), 0);
        assert_eq!(counter.get_count_till(16 * 2 + 1), 0);

        assert_eq!(counter.get_count_till(16 * 3), 0);
        assert_eq!(counter.get_count_till(16 * 3 + 1), 0);
    }

    #[test]
    fn test_increment_check_bucket() {
        const BUCKET_COUNT: usize = 3;
        const SHIFT_FACTOR: u32 = 2;

        let counter = Counter::<BUCKET_COUNT, 2>::new(SHIFT_FACTOR);
        // ----------   ----------   ----------
        // | (0, 0) |   | (0, 0) |   | (0, 0) |
        // | (0, 0) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11

        counter.increment_by_one(0);
        // ----------   ----------   ----------
        // | (0, 0) |   | (0, 0) |   | (0, 0) |
        // | (0, 1) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 0);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 0);
        assert_eq!(a.value, 0);
        drop(a);

        counter.increment_by_one(1);
        // ----------   ----------   ----------
        // | (1, 1) |   | (0, 0) |   | (0, 0) |
        // | (0, 1) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 0);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 1);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(2);
        // ----------   ----------   ----------
        // | (1, 1) |   | (0, 0) |   | (0, 0) |
        // | (2, 2) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 2);
        assert_eq!(a.value, 2);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 1);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(3);
        // ----------   ----------   ----------
        // | (3, 2) |   | (0, 0) |   | (0, 0) |
        // | (2, 2) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 2);
        assert_eq!(a.value, 2);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 3);
        assert_eq!(a.value, 2);
        drop(a);

        counter.increment_by_one(4);
        // ----------   ----------   ----------
        // | (3, 2) |   | (0, 0) |   | (0, 0) |
        // | (2, 2) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[1][0].lock().unwrap();
        assert_eq!(a.key, 4);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[1][1].lock().unwrap();
        assert_eq!(a.key, 0);
        assert_eq!(a.value, 0);
        drop(a);

        counter.increment_by_one(5);
        // ----------   ----------   ----------
        // | (3, 2) |   | (5, 1) |   | (0, 0) |
        // | (2, 2) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[1][0].lock().unwrap();
        assert_eq!(a.key, 4);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[1][1].lock().unwrap();
        assert_eq!(a.key, 5);
        assert_eq!(a.value, 1);
        drop(a);

        // Almost at the end of the ring buffer

        counter.increment_by_one(11);
        // ----------   ----------   ----------
        // | (3, 2) |   | (5, 1) |   |(11, 1) |
        // | (2, 2) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[2][0].lock().unwrap();
        assert_eq!(a.key, 0);
        assert_eq!(a.value, 0);
        drop(a);
        let a = counter.buckets[2][1].lock().unwrap();
        assert_eq!(a.key, 11);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(12);
        // ----------   ----------   ----------
        // | (3, 2) |   | (5, 1) |   |(11, 1) |
        // |(12, 1) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 12);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 3);
        assert_eq!(a.value, 2);
        drop(a);

        counter.increment_by_one(13);
        // ----------   ----------   ----------
        // |(13, 1) |   | (5, 1) |   |(11, 1) |
        // |(12, 1) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 12);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 13);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(14);
        // ----------   ----------   ----------
        // |(13, 1) |   | (5, 1) |   |(11, 1) |
        // |(14, 2) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 14);
        assert_eq!(a.value, 2);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 13);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(15);
        // ----------   ----------   ----------
        // |(15, 2) |   | (5, 1) |   |(11, 1) |
        // |(14, 2) |   | (4, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 14);
        assert_eq!(a.value, 2);
        drop(a);
        let a = counter.buckets[0][1].lock().unwrap();
        assert_eq!(a.key, 15);
        assert_eq!(a.value, 2);
        drop(a);

        counter.increment_by_one(16);
        // ----------   ----------   ----------
        // |(15, 2) |   | (5, 1) |   |(11, 1) |
        // |(14, 2) |   |(16, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //    0-3          4-7          8-11
        let a = counter.buckets[1][0].lock().unwrap();
        assert_eq!(a.key, 16);
        assert_eq!(a.value, 1);
        drop(a);
        let a = counter.buckets[1][1].lock().unwrap();
        assert_eq!(a.key, 5);
        assert_eq!(a.value, 1);
        drop(a);
    }

    #[test]
    fn test_get_count_till() {
        const BUCKET_COUNT: usize = 3;
        const SHIFT_FACTOR: u32 = 2;

        let counter = Counter::<BUCKET_COUNT, 2>::new(SHIFT_FACTOR);

        counter.increment_by_one(0);
        assert_eq!(counter.get_count_till(0), 1);

        counter.increment_by_one(0);
        assert_eq!(counter.get_count_till(0), 2);

        counter.increment_by_one(1);
        assert_eq!(counter.get_count_till(1), 3);

        counter.increment_by_one(1);
        assert_eq!(counter.get_count_till(1), 4);

        counter.increment_by_one(2);
        assert_eq!(counter.get_count_till(2), 5);

        counter.increment_by_one(3);
        assert_eq!(counter.get_count_till(3), 6);

        counter.increment_by_one(4);
        assert_eq!(counter.get_count_till(4), 7);

        counter.increment_by_one(5);
        assert_eq!(counter.get_count_till(5), 8);

        counter.increment_by_one(6);
        assert_eq!(counter.get_count_till(6), 9);

        counter.increment_by_one(7);
        assert_eq!(counter.get_count_till(7), 10);

        counter.increment_by_one(8);
        assert_eq!(counter.get_count_till(8), 11);

        counter.increment_by_one(11);
        assert_eq!(counter.get_count_till(11), 12);
    }

    #[test]
    fn test_get_count_till_cycle() {
        const BUCKET_COUNT: usize = 3;
        const SHIFT_FACTOR: u32 = 2;

        let counter = Counter::<BUCKET_COUNT, 2>::new(SHIFT_FACTOR);

        counter.increment_by_one(0);
        assert_eq!(counter.get_count_till(0), 1);

        counter.increment_by_one(1);
        assert_eq!(counter.get_count_till(1), 2);

        counter.increment_by_one(11);
        assert_eq!(counter.get_count_till(11), 3);

        counter.increment_by_one(12);
        assert_eq!(counter.get_count_till(12), 3);

        counter.increment_by_one(13);
        assert_eq!(counter.get_count_till(13), 3);

        counter.increment_by_one(14);
        assert_eq!(counter.get_count_till(14), 4);
    }

    #[test]
    fn test_get_count_till_expired() {
        const BUCKET_COUNT: usize = 3;
        const SHIFT_FACTOR: u32 = 2;

        let counter = Counter::<BUCKET_COUNT, 2>::new(SHIFT_FACTOR);

        counter.increment_by_one(0);
        assert_eq!(counter.get_count_till(0), 1);

        counter.increment_by_one(11);
        assert_eq!(counter.get_count_till(11), 2);

        assert_eq!(counter.get_count_till(1_000), 0);
    }

    #[test]
    fn test_increment_check_bucket_shift_factor_0() {
        const BUCKET_COUNT: usize = 3;
        const SHIFT_FACTOR: u32 = 0;

        let counter = Counter::<BUCKET_COUNT, 1>::new(SHIFT_FACTOR);
        // ----------   ----------   ----------
        // | (0, 0) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //     3            4            8

        counter.increment_by_one(0);
        // ----------   ----------   ----------
        // | (0, 1) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //     3            4            8
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 0);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(1);
        // ----------   ----------   ----------
        // | (0, 1) |   | (1, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        //     3            4            8
        let a = counter.buckets[1][0].lock().unwrap();
        assert_eq!(a.key, 1);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(2);
        // ----------   ----------   ----------
        // | (0, 1) |   | (1, 1) |   | (2, 1) |
        // ----------   ----------   ----------
        //     0            1            2
        //     3            4            8
        let a = counter.buckets[2][0].lock().unwrap();
        assert_eq!(a.key, 2);
        assert_eq!(a.value, 1);
        drop(a);

        counter.increment_by_one(3);
        // ----------   ----------   ----------
        // | (3, 1) |   | (1, 1) |   | (2, 1) |
        // ----------   ----------   ----------
        //     0            1            2
        //     3            4            8
        let a = counter.buckets[0][0].lock().unwrap();
        assert_eq!(a.key, 3);
        assert_eq!(a.value, 1);
        drop(a);
    }

    #[test]
    fn test_shift_factor_0() {
        const BUCKET_COUNT: usize = 3;
        const SHIFT_FACTOR: u32 = 0;

        let counter = Counter::<BUCKET_COUNT, 1>::new(SHIFT_FACTOR);
        // ----------   ----------   ----------
        // | (0, 0) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2

        counter.increment_by_one(0);
        // ----------   ----------   ----------
        // | (0, 1) |   | (0, 0) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        assert_eq!(counter.get_count_till(0), 1);

        counter.increment_by_one(1);
        // ----------   ----------   ----------
        // | (0, 1) |   | (1, 1) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        assert_eq!(counter.get_count_till(1), 2);

        counter.increment_by_one(1);
        // ----------   ----------   ----------
        // | (0, 1) |   | (1, 2) |   | (0, 0) |
        // ----------   ----------   ----------
        //     0            1            2
        assert_eq!(counter.get_count_till(1), 3);

        counter.increment_by_one(2);
        // ----------   ----------   ----------
        // | (0, 1) |   | (1, 2) |   | (2, 1) |
        // ----------   ----------   ----------
        //     0            1            2
        assert_eq!(counter.get_count_till(2), 4);

        counter.increment_by_one(3);
        // ----------   ----------   ----------
        // | (3, 1) |   | (1, 2) |   | (2, 1) |
        // ----------   ----------   ----------
        //     0            1            2
        assert_eq!(counter.get_count_till(3), 4);
    }

    #[test]
    fn test_parallel() {
        for _ in 0..100 {
            let counter = Counter::<3, 5>::new(0);
            let counter = Arc::new(counter);

            const THREAD_NUMBER: usize = 2;

            let ths: Vec<_> = (0..THREAD_NUMBER)
                .map(|_| {
                    let counter = Arc::clone(&counter);
                    thread::spawn(move || {
                        counter.increment_by_one(0);
                        counter.increment_by_one(1);
                        counter.increment_by_one(2);
                        counter.increment_by_one(3);
                    })
                })
                .collect();

            for th in ths {
                th.join().unwrap();
            }

            assert_eq!(counter.get_count_till(0), THREAD_NUMBER);
            assert_eq!(counter.get_count_till(1), 2 * THREAD_NUMBER);
            assert_eq!(counter.get_count_till(2), 3 * THREAD_NUMBER);
            assert_eq!(counter.get_count_till(3), 3 * THREAD_NUMBER); // 0 is forgotten
        }
    }
}
