use alloc::vec::Vec;
use core::time::Duration;

use super::QueueDepth;

const PERIOD: u64 = 16_666_667;
/// A drawable presented 3 ms before a refresh, in time for the composite.
const IN_TIME: u64 = 2 * PERIOD + 3_000_000;
/// A drawable that waited a period more in the queue.
const LATE: u64 = 3 * PERIOD + 2_000_000;

/// A layer that presents one drawable each refresh period, and whose
/// display shows at most one drawable each period.
struct Layer {
    depth: QueueDepth,
    id: u64,
    at_ns: u64,
    shown_ns: Option<u64>,
}

impl Layer {
    fn new() -> Self {
        Self {
            depth: QueueDepth::default(),
            id: 0,
            at_ns: 0,
            shown_ns: None,
        }
    }

    /// Acquires, presents and shows the next drawable `delay` after its
    /// present, but no sooner than a period after the last show, or never
    /// when `delay` is `None`. The acquire's wait, if any, moves this frame
    /// and the ones after it later.
    fn frame(&mut self, delay: Option<u64>) -> Option<Duration> {
        let wait = self.depth.wait_before_acquire(self.at_ns);
        self.at_ns += wait.map_or(0, |wait| wait.as_nanos() as u64);
        self.depth.presented(self.id, self.at_ns);
        let shown = delay.map(|delay| {
            let earliest = self.shown_ns.map_or(0, |last| last + PERIOD);
            (self.at_ns + delay).max(earliest)
        });
        self.shown_ns = shown.or(self.shown_ns);
        self.depth.shown(self.id, shown);
        self.id += 1;
        self.at_ns += PERIOD;
        wait
    }

    /// A period with no present, as a slow frame leaves.
    fn pause(&mut self) {
        self.at_ns += PERIOD;
    }

    fn frames(&mut self, count: usize, delay: u64) -> Vec<Option<Duration>> {
        (0..count).map(|_| self.frame(Some(delay))).collect()
    }
}

fn waited(waits: &[Option<Duration>]) -> usize {
    waits.iter().flatten().count()
}

#[test]
fn a_queue_that_shows_each_drawable_in_time_never_waits() {
    let mut layer = Layer::new();
    assert_eq!(waited(&layer.frames(120, IN_TIME)), 0);
}

#[test]
fn three_late_drawables_in_a_row_make_the_next_acquire_wait_one_period() {
    let mut layer = Layer::new();
    // The first show gives no period yet; the next three are late.
    assert_eq!(waited(&layer.frames(4, LATE)), 0);
    let wait = layer.frame(Some(IN_TIME));
    let period = wait.expect("the acquire after three late drawables waits");
    assert!(period.as_nanos().abs_diff(u128::from(PERIOD)) < 2);
}

#[test]
fn a_drawable_in_time_starts_the_count_over() {
    let mut layer = Layer::new();
    let mut waits = layer.frames(3, LATE);
    layer.pause();
    waits.push(layer.frame(Some(IN_TIME)));
    waits.extend(layer.frames(2, LATE));
    layer.pause();
    waits.push(layer.frame(Some(IN_TIME)));
    assert_eq!(waited(&waits), 0);
}

#[test]
fn a_wait_that_shortens_the_queue_needs_three_new_late_drawables_for_the_next() {
    let mut layer = Layer::new();
    layer.frames(4, LATE);
    assert!(layer.frame(Some(IN_TIME)).is_some());
    assert_eq!(waited(&layer.frames(3, LATE)), 0);
    assert!(layer.frame(Some(IN_TIME)).is_some());
}

/// Brings `layer` to a wait that gives nothing back: three late drawables,
/// a wait, and three late drawables after it.
fn wait_in_vain(layer: &mut Layer) {
    assert_eq!(waited(&layer.frames(4, LATE)), 0);
    assert!(layer.frame(Some(LATE)).is_some());
    assert_eq!(waited(&layer.frames(2, LATE)), 0);
}

#[test]
fn a_wait_that_gave_nothing_back_rests_sixty_shows_before_the_next() {
    let mut layer = Layer::new();
    wait_in_vain(&mut layer);
    assert_eq!(waited(&layer.frames(60 + 3, LATE)), 0);
    assert!(layer.frame(Some(LATE)).is_some());
}

#[test]
fn each_wait_that_gave_nothing_back_doubles_the_rest() {
    let mut layer = Layer::new();
    wait_in_vain(&mut layer);
    layer.frames(60 + 3, LATE);
    assert!(layer.frame(Some(LATE)).is_some());
    assert_eq!(waited(&layer.frames(2, LATE)), 0);
    assert_eq!(waited(&layer.frames(120 + 3, LATE)), 0);
    assert!(layer.frame(Some(LATE)).is_some());
}

#[test]
fn a_wait_that_shortens_the_queue_starts_the_rest_over() {
    let mut layer = Layer::new();
    wait_in_vain(&mut layer);
    layer.frames(60 + 3, LATE);
    assert!(layer.frame(Some(IN_TIME)).is_some());
    // The next wait in vain rests sixty shows again, not a hundred and twenty.
    layer.frames(3, LATE);
    assert!(layer.frame(Some(LATE)).is_some());
    layer.frames(2, LATE);
    assert_eq!(waited(&layer.frames(60 + 3, LATE)), 0);
    assert!(layer.frame(Some(LATE)).is_some());
}

#[test]
fn drawables_presented_before_the_wait_tell_nothing_of_it() {
    let mut depth = QueueDepth::default();
    // Shows reach the layer three frames after their presents.
    for id in 0..7 {
        depth.presented(id, id * PERIOD);
        if id >= 3 {
            let shown = id - 3;
            depth.shown(shown, Some(shown * PERIOD + LATE));
        }
    }
    depth.shown(3, Some(3 * PERIOD + LATE));
    let since = 7 * PERIOD;
    assert!(depth.wait_before_acquire(since).is_some());
    for id in 4..7 {
        depth.shown(id, Some(id * PERIOD + LATE));
    }
    // A drawable after the wait shows in time: the queue got shorter, and
    // three late drawables still count as late.
    depth.presented(7, since + PERIOD);
    depth.shown(7, Some(since + PERIOD + IN_TIME));
    for id in 8..11 {
        let at = since + (id - 6) * PERIOD;
        depth.presented(id, at);
        depth.shown(id, Some(at + LATE));
    }
    assert!(depth.wait_before_acquire(since + 5 * PERIOD).is_some());
}

#[test]
fn an_acquire_after_a_pause_waits_for_nothing() {
    let mut layer = Layer::new();
    layer.frames(4, LATE);
    layer.pause();
    layer.pause();
    assert_eq!(layer.frame(Some(IN_TIME)), None);
    assert_eq!(layer.frame(Some(IN_TIME)), None);
}

#[test]
fn a_drawable_that_never_showed_leaves_the_count_as_it_was() {
    let mut layer = Layer::new();
    let mut waits = layer.frames(3, LATE);
    waits.push(layer.frame(None));
    waits.push(layer.frame(Some(LATE)));
    assert_eq!(waited(&waits), 0);
    assert!(layer.frame(Some(IN_TIME)).is_some());
}

#[test]
fn a_new_configuration_starts_over() {
    let mut layer = Layer::new();
    layer.frames(4, LATE);
    layer.depth = QueueDepth::default();
    assert_eq!(waited(&layer.frames(3, LATE)), 0);
}
