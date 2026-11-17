use std::{sync::Arc, thread};

use super::*;

fn unit(pts_us: u64) -> PendingUnit {
    PendingUnit {
        pts_us,
        timestamp: Duration::from_micros(pts_us),
        metadata: pts_us,
        submitted: Instant::now(),
    }
}

// A decoder's shared state that holds `waiting` units for their pictures, at timestamps from 0.
fn shared(waiting: u64) -> Shared {
    let shared = Shared {
        pending: Mutex::new(PendingUnits::new()),
        room: Condvar::new(),
        queue: Arc::new(FrameQueue::new(2, 0)),
        live: LivePictures::default(),
        depth: 2,
        session_lost: AtomicBool::new(false),
    };
    for pts_us in 0..waiting {
        shared.pending.lock().push(unit(pts_us));
    }
    shared
}

#[test]
fn a_zero_wait_refuses_a_unit_that_has_no_room_and_counts_it() {
    let shared = shared(MAX_IN_FLIGHT as u64);
    let refused = shared.reserve(unit(100), Duration::ZERO);
    assert!(matches!(refused, Err(DecoderError::Full)));
    assert_eq!(shared.queue.stats.snapshot().refused, 1);
    // The refused unit is not in the decoder, so no picture will come out of it
    assert_eq!(shared.pending.lock().len(), MAX_IN_FLIGHT);
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test gives the waiting thread time to block before the callback makes room"
)]
fn a_unit_takes_the_place_that_the_callback_makes_while_it_waits() {
    let shared = Arc::new(shared(MAX_IN_FLIGHT as u64));
    let callback = Arc::clone(&shared);
    let retire = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        callback.withdraw(0);
    });
    shared
        .reserve(unit(100), Duration::from_secs(10))
        .expect("room after the callback retires a unit");
    retire.join().expect("the callback thread");
    assert_eq!(shared.queue.stats.snapshot().refused, 0);
    assert_eq!(shared.pending.lock().len(), MAX_IN_FLIGHT);
}
