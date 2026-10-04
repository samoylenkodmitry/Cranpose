use std::alloc::System;

use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "support/blocking.rs"]
mod support;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn main() {
    support::serial(1);
    let region = Region::new(GLOBAL);
    let checksum = support::serial(1_000);
    let change = region.change();
    println!(
        "{{\"phase\":\"allocations\",\"jobs\":1000,\"checksum\":{checksum},\"allocations\":{},\"bytes_allocated\":{},\"deallocations\":{}}}",
        change.allocations, change.bytes_allocated, change.deallocations
    );
}
