use invocation_counter::Counter;

fn main() {
    const BUCKET_COUNT: usize = 16;
    const SUB_BUCKET_COUNT: usize = 2;
    const GROUP_SHIFT_FACTOR: u32 = 4;
    // 4 is the group_shift_factor
    // 16 is the number of buckets
    let counter = Counter::<BUCKET_COUNT, SUB_BUCKET_COUNT>::new(GROUP_SHIFT_FACTOR);

    // Typically you want to use something like `Instant::now().elapsed().as_secs()`
    let mut now: u64 = 0;
    counter.increment_by_one(now);

    now += 1; // Simulate a second passing
    counter.increment_by_one(now);

    assert_eq!(counter.get_count_till(now), 2);

    now += 2_u64.pow(GROUP_SHIFT_FACTOR) * BUCKET_COUNT as u64; // Move forward...
    counter.increment_by_one(now);
    // The counter forgot about the counts older than 2 ** 4 * 16 seconds
    assert_eq!(counter.get_count_till(now), 1);
}
