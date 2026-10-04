use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static ALLOCATOR: &StatsAlloc<std::alloc::System> = &INSTRUMENTED_SYSTEM;

#[path = "support/workers.rs"]
mod support;

fn main() {
    support::serial(32);
    pollster::block_on(support::blocking(|| 1));
    let measured = Region::new(ALLOCATOR);
    let sum = support::serial(1_000);
    let counts = measured.change();
    assert_eq!(sum, 499_500);
    println!(
        "{{\"mode\":\"allocations\",\"checksum\":{sum},\"count\":{},\"bytes\":{}}}",
        counts.allocations, counts.bytes_allocated
    );
}
