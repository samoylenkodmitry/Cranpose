// The newest sample, shared between the sensor thread and every reader.
//
// It is a sequence lock made of atomics. The writer makes the sequence odd,
// stores the fields, and makes it even again. A reader repeats its loads
// until it saw the same even sequence before and after them. Readers never
// write and never wait on a lock, so a reader costs the same on any thread.

use std::sync::atomic::{AtomicU32, Ordering, fence};

use portable_atomic::AtomicU64;

use super::{
    quaternion::Quaternion,
    sample::{MotionClock, MotionReference, MotionSample},
};

// The sensor is running: a published sample is kept.
const ACCEPTING: u32 = 1;
// The fields hold a sample.
const HOLDS_SAMPLE: u32 = 1 << 1;
const MAGNETIC: u32 = 1 << 2;
const CLOCK_SHIFT: u32 = 3;
const CLOCK_MASK: u32 = 0b11;

// How many times a reader tries again before it yields its time slice to a
// writer that was descheduled in the middle of its stores.
const SPINS_BEFORE_YIELD: u32 = 64;

pub(super) struct LatestSample {
    sequence: AtomicU32,
    rotation: [AtomicU32; 4],
    timestamp: AtomicU64,
    index: AtomicU64,
    flags: AtomicU32,
}

fn encode(reference: MotionReference, clock: MotionClock) -> u32 {
    let reference = match reference {
        MotionReference::ArbitraryHeading => 0,
        MotionReference::MagneticNorth => MAGNETIC,
    };
    let clock = match clock {
        MotionClock::BootTime => 0,
        MotionClock::SystemUptime => 1,
        MotionClock::PerformanceNow => 2,
    };
    ACCEPTING | HOLDS_SAMPLE | reference | (clock << CLOCK_SHIFT)
}

fn decode_clock(flags: u32) -> MotionClock {
    match (flags >> CLOCK_SHIFT) & CLOCK_MASK {
        1 => MotionClock::SystemUptime,
        2 => MotionClock::PerformanceNow,
        _ => MotionClock::BootTime,
    }
}

impl LatestSample {
    pub(super) const fn new() -> Self {
        Self {
            sequence: AtomicU32::new(0),
            rotation: [
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
            ],
            timestamp: AtomicU64::new(0),
            index: AtomicU64::new(0),
            flags: AtomicU32::new(0),
        }
    }

    // Makes the sequence odd, which also keeps a second writer out.
    fn lock(&self) -> u32 {
        loop {
            let before = self.sequence.load(Ordering::Relaxed);
            if before & 1 == 0
                && self
                    .sequence
                    .compare_exchange_weak(
                        before,
                        before.wrapping_add(1),
                        Ordering::Acquire,
                        Ordering::Relaxed,
                    )
                    .is_ok()
            {
                // The stores of the fields must not become visible before the
                // odd sequence does.
                fence(Ordering::Release);
                return before;
            }
            std::hint::spin_loop();
        }
    }

    fn unlock(&self, before: u32) {
        self.sequence
            .store(before.wrapping_add(2), Ordering::Release);
    }

    // Starts keeping samples, from index one.
    pub(super) fn begin(&self) {
        let before = self.lock();
        self.index.store(0, Ordering::Relaxed);
        self.flags.store(ACCEPTING, Ordering::Relaxed);
        self.unlock(before);
    }

    // Stops keeping samples and forgets the last one.
    pub(super) fn end(&self) {
        let before = self.lock();
        self.flags.store(0, Ordering::Relaxed);
        self.unlock(before);
    }

    // Forgets the last sample and keeps accepting.
    pub(super) fn interrupt(&self) {
        let before = self.lock();
        let flags = self.flags.load(Ordering::Relaxed);
        self.flags.store(flags & ACCEPTING, Ordering::Relaxed);
        self.unlock(before);
    }

    // Keeps `rotation` as the newest sample and returns whether it was kept.
    // A sample that arrives while the sensor is stopped is dropped here, so
    // a late delivery cannot bring a stopped sensor's sample back.
    pub(super) fn publish(
        &self,
        rotation: Quaternion,
        timestamp_ns: u64,
        reference: MotionReference,
        clock: MotionClock,
    ) -> bool {
        let before = self.lock();
        let accepting = self.flags.load(Ordering::Relaxed) & ACCEPTING != 0;
        if accepting {
            for (cell, component) in self
                .rotation
                .iter()
                .zip([rotation.x, rotation.y, rotation.z, rotation.w])
            {
                cell.store(component.to_bits(), Ordering::Relaxed);
            }
            self.timestamp.store(timestamp_ns, Ordering::Relaxed);
            let index = self.index.load(Ordering::Relaxed).wrapping_add(1);
            self.index.store(index, Ordering::Relaxed);
            self.flags
                .store(encode(reference, clock), Ordering::Relaxed);
        }
        self.unlock(before);
        accepting
    }

    // The newest sample, or `None` while the sensor is stopped or has not
    // reported since it started.
    pub(super) fn read(&self) -> Option<MotionSample> {
        let mut attempts = 0u32;
        loop {
            let before = self.sequence.load(Ordering::Acquire);
            if before & 1 == 0 {
                let [x, y, z, w] = [0, 1, 2, 3]
                    .map(|at| f32::from_bits(self.rotation[at].load(Ordering::Relaxed)));
                let timestamp_ns = self.timestamp.load(Ordering::Relaxed);
                let index = self.index.load(Ordering::Relaxed);
                let flags = self.flags.load(Ordering::Relaxed);
                // The loads above must not move below the sequence check.
                fence(Ordering::Acquire);
                if self.sequence.load(Ordering::Relaxed) == before {
                    return (flags & HOLDS_SAMPLE != 0).then(|| MotionSample {
                        rotation: Quaternion::new(x, y, z, w),
                        timestamp_ns,
                        clock: decode_clock(flags),
                        reference: if flags & MAGNETIC != 0 {
                            MotionReference::MagneticNorth
                        } else {
                            MotionReference::ArbitraryHeading
                        },
                        index,
                    });
                }
            }
            attempts += 1;
            if attempts > SPINS_BEFORE_YIELD {
                std::thread::yield_now();
            } else {
                std::hint::spin_loop();
            }
        }
    }
}
