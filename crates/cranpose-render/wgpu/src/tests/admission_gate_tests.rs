use cranpose_render_common::raster_cache::RasterScale;
use cranpose_ui_graphics::Rect;

use super::*;

#[test]
fn a_gate_admits_a_key_that_held_for_more_than_its_patience() {
    let key = gate_key(1);
    let mut gate = AdmissionGate::copied(key);
    assert!(!gate.admits(), "a key seen once is only remembered");
    gate.observe(key);
    assert!(gate.admits(), "the second frame of a key admits it");
    gate.admitted();
    gate.hit(key);
    assert_eq!(patience(&gate), 1);
    assert!(gate.end_frame(), "a gate seen this frame stays");
    assert!(!gate.end_frame(), "a gate not seen since goes");
}

#[test]
fn a_cached_key_between_misses_breaks_the_other_keys_consecutive_run() {
    let first = gate_key(1);
    let other = gate_key(2);
    let mut gate = AdmissionGate::copied(first);
    gate.observe(first);
    assert!(gate.admits());
    gate.admitted();
    gate.observe(other);
    assert!(!gate.admits());
    gate.observe(other);
    assert!(!gate.admits());
    gate.hit(first);
    gate.observe(other);
    assert!(
        !gate.admits(),
        "the other key has held for only one frame since the cache hit"
    );
}
/// One frame of a node's surface: read from the cache while its kept copy
/// is current, otherwise offered to the gate.
fn gate_frame(gate: &mut AdmissionGate, key: LayerRasterCacheKey) -> bool {
    if gate.admitted && gate.key == key {
        gate.hit(key);
        return false;
    }
    gate.observe(key);
    if gate.admits() {
        gate.admitted();
        return true;
    }
    false
}

fn admissions_over(gate: &mut AdmissionGate, holds: impl IntoIterator<Item = u32>) -> u32 {
    let mut admissions = 0;
    for (step, hold) in holds.into_iter().enumerate() {
        for _ in 0..hold {
            admissions += u32::from(gate_frame(gate, gate_key(step as u64 + 1)));
        }
    }
    admissions
}

#[test]
fn a_gate_waits_twice_as_long_after_an_admission_nothing_read_back() {
    let mut gate = AdmissionGate::copied(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(2, 40)),
        1,
        "a key that never holds a third frame is admitted once"
    );
    assert_eq!(patience(&gate), 2);
    let mut gate = AdmissionGate::copied(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(4, 12)),
        12,
        "a key that holds a fourth frame is read back twice per admission"
    );
    assert_eq!(
        patience(&gate),
        1,
        "an admission read back twice does not double the patience"
    );
}

/// A grid resizing a pixel at a time holds each cell's size two or three
/// frames. A copy kept on its second frame is read once on its third and
/// replaced: it costs a copy and a texture for one read, so the gate waits
/// longer instead of keeping the next one.
#[test]
fn a_copied_gate_stops_keeping_what_is_read_only_once() {
    let mut gate = AdmissionGate::copied(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(3, 40)),
        2,
        "a key held three frames is kept until reading it once proves too little"
    );
    assert_eq!(patience(&gate), 4);
    assert_eq!(
        admissions_over(&mut gate, [40]),
        1,
        "content that holds still is still kept"
    );
    assert_eq!(
        patience(&gate),
        2,
        "a copy read back twice halves the wait, so the churn after a pause stays out"
    );
}

#[test]
fn a_pinned_gate_admits_every_uncached_frame_and_counts_the_hold() {
    let mut gate = AdmissionGate::pinned(gate_key(0));
    assert!(gate.admits(), "a pin costs no pass, so first sight admits");
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(2, 40)),
        40,
        "every two-frame hold is pinned on its first frame and replayed on its second"
    );
    assert_eq!(
        gate.run, 2,
        "the replay counted as a second frame of the hold"
    );
    let mut gate = AdmissionGate::pinned(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(1, 40)),
        40,
        "an unread pin costs nothing to repeat, so a key changing every frame is pinned \
         every frame"
    );
    assert_eq!(gate.run, 1);
    for _ in 0..4 {
        gate.observe(gate_key(99));
    }
    assert_eq!(
        gate.run, 4,
        "a held key's run is what the admission budget ranks by"
    );
}

#[test]
fn a_pin_lives_exactly_as_long_as_its_key_and_a_copy_only_dies_unread() {
    let mut gate = AdmissionGate::pinned(gate_key(1));
    assert_eq!(
        gate.dead_entry(),
        None,
        "nothing admitted, nothing to hand back"
    );
    gate.admitted();
    assert_eq!(gate.dead_entry(), Some(gate_key(1)));
    assert_eq!(
        gate.observe(gate_key(1)),
        None,
        "the same key holds the pin"
    );
    assert_eq!(
        gate.observe(gate_key(2)),
        Some(gate_key(1)),
        "a pin nothing read back dies with its key"
    );
    assert_eq!(gate.dead_entry(), None);
    gate.admitted();
    gate.hit(gate_key(2));
    assert_eq!(
        gate.observe(gate_key(3)),
        Some(gate_key(2)),
        "a pin that was read back dies with its key too: a re-pin costs nothing"
    );
    let mut gate = AdmissionGate::copied(gate_key(1));
    gate.observe(gate_key(1));
    gate.admitted();
    gate.hit(gate_key(1));
    assert_eq!(
        gate.observe(gate_key(2)),
        None,
        "a copy that was read back stays for the cache to keep or evict"
    );
    gate.observe(gate_key(2));
    gate.admitted();
    assert_eq!(
        gate.observe(gate_key(3)),
        Some(gate_key(2)),
        "a copy nothing read back is handed back"
    );
}

fn patience(gate: &AdmissionGate) -> u32 {
    match gate.cost {
        AdmissionCost::Pin => 0,
        AdmissionCost::Copy { patience, .. } => patience,
    }
}

#[test]
fn a_rendered_gate_admits_a_first_sight_and_waits_after_an_unread_admission() {
    let mut gate = AdmissionGate::rendered(gate_key(0));
    assert!(
        gate.admits(),
        "a surface seen for the first time is kept, as still content reads it back next frame"
    );
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(1, 40)),
        1,
        "a surface that changes every frame is kept once, then drawn without being stored"
    );
    assert_eq!(patience(&gate), 1);
    assert_eq!(
        admissions_over(&mut gate, [4]),
        1,
        "a surface that settles is kept on its second frame"
    );
    assert_eq!(
        patience(&gate),
        0,
        "a kept surface read back twice restores first-sight admission"
    );
    assert!(gate_frame(&mut gate, gate_key(100)));
}

#[test]
fn a_gate_never_waits_longer_than_the_cap() {
    let mut gate = AdmissionGate::copied(gate_key(0));
    let admissions = admissions_over(&mut gate, [2, 3, 5, 9, 17, 17, 17]);
    assert_eq!(
        admissions, 7,
        "each hold one frame past the patience is admitted on its last frame"
    );
    assert_eq!(patience(&gate), MAX_ADMISSION_PATIENCE);
}

#[test]
fn an_in_place_gate_waits_longer_after_the_surface_it_kept_went_unread() {
    let mut gate = AdmissionGate::drawn_in_place(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(IN_PLACE_PATIENCE + 1, 6)),
        1,
        "a relayout pausing one frame past the patience at every turn of its motion keeps \
         its surface once, and not again after nothing read it"
    );
    assert!(patience(&gate) > IN_PLACE_PATIENCE);
    // Held one frame past the patience to be kept, then read twice.
    let settles = patience(&gate) + 3;
    assert_eq!(
        admissions_over(&mut gate, [settles]),
        1,
        "content that settles is still kept"
    );
    assert_eq!(
        patience(&gate),
        IN_PLACE_PATIENCE,
        "a kept surface read back twice restores the patience"
    );
}

#[test]
fn an_in_place_gate_never_waits_longer_than_its_cap() {
    let mut gate = AdmissionGate::drawn_in_place(gate_key(0));
    let holds = [17, 33, 65, 129, 257, 257, 257];
    assert_eq!(
        admissions_over(&mut gate, holds),
        7,
        "each hold one frame past the patience is admitted on its last frame"
    );
    assert_eq!(patience(&gate), IN_PLACE_MAX_PATIENCE);
}

fn gate_key(content: u64) -> LayerRasterCacheKey {
    LayerRasterCacheKey::backdrop_effect(
        None,
        content,
        0,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        (1, 1),
        RasterScale::from_scale(1.0),
    )
}
