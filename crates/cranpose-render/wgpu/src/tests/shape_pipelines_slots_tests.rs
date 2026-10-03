use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

use cranpose_ui_graphics::BlendMode;
use web_time::{Duration, Instant};

use super::{KeyedBuild, Slots};
use crate::{
    pipeline_compiler::PipelineCompiler,
    render::{RunTier, ShapePipelineKey},
};

fn key(blend: BlendMode) -> ShapePipelineKey {
    ShapePipelineKey::general_for(blend, RunTier::Store)
}

/// Builds a key's blend mode, counting builds, each waiting for a release
/// once it has said it started.
struct GatedBuilder {
    builds: Arc<AtomicUsize>,
    started: Mutex<mpsc::Sender<ShapePipelineKey>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl KeyedBuild for GatedBuilder {
    type Output = BlendMode;

    fn build(&self, key: ShapePipelineKey) -> BlendMode {
        self.builds.fetch_add(1, Ordering::SeqCst);
        let _ = self.started.lock().expect("started sender").send(key);
        let _ = self
            .release
            .lock()
            .expect("release receiver")
            .recv_timeout(Duration::from_secs(5));
        key.blend_mode
    }
}

struct Gate {
    builds: Arc<AtomicUsize>,
    started: mpsc::Receiver<ShapePipelineKey>,
    release: mpsc::Sender<()>,
}

fn gated() -> (GatedBuilder, Gate) {
    let builds = Arc::new(AtomicUsize::new(0));
    let (started_tx, started) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    (
        GatedBuilder {
            builds: Arc::clone(&builds),
            started: Mutex::new(started_tx),
            release: Mutex::new(release_rx),
        },
        Gate {
            builds,
            started,
            release,
        },
    )
}

#[test]
fn a_draw_waits_for_the_warm_up_building_its_value_instead_of_building_again() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let warmed = key(BlendMode::DstOut);
    slots.warm(warmed);
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(warmed),
        "the warm-up starts without a draw"
    );
    let release = gate.release.clone();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = release.send(());
    });
    let need = slots.need(warmed);
    assert!(need.first && !need.ready);
    slots.build(warmed);
    assert!(releaser.join().is_ok());
    assert_eq!(slots.get(warmed), Some(&BlendMode::DstOut));
    assert_eq!(gate.builds.load(Ordering::SeqCst), 1, "built once");
    assert!(!slots.need(warmed).first, "a key is new to draws once");
}

#[test]
fn an_inactive_compiler_warms_nothing_and_draws_build_where_they_ask() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::inactive(), builder);
    let warmed = key(BlendMode::SrcOver);
    slots.warm(warmed);
    assert_eq!(gate.builds.load(Ordering::SeqCst), 0);
    assert!(gate.release.send(()).is_ok());
    slots.build(warmed);
    assert_eq!(slots.get(warmed), Some(&BlendMode::SrcOver));
    assert_eq!(gate.builds.load(Ordering::SeqCst), 1);
}

#[test]
fn a_draw_can_start_its_pipeline_ahead_of_unrelated_warm_ups() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let background = key(BlendMode::DstOut);
    let drawn = key(BlendMode::SrcOver);
    slots.warm(background);
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(background)
    );
    slots.warm(drawn);
    slots.want(drawn, 4);
    slots.request_wanted();
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(drawn),
        "a draw must not wait behind an unrelated warm-up"
    );
    for _ in 0..2 {
        assert!(gate.release.send(()).is_ok());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while slots.get(background).is_none() || slots.get(drawn).is_none() {
        assert!(Instant::now() < deadline, "pipelines did not finish");
        std::thread::yield_now();
    }
    assert_eq!(gate.builds.load(Ordering::SeqCst), 2, "each compiled once");
}

#[test]
fn a_frame_queues_two_of_its_wants_and_the_rest_wait_for_the_next_frame() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let first = key(BlendMode::SrcOver);
    let second = key(BlendMode::DstOut);
    let third = key(BlendMode::Plus);
    slots.want(first, 300);
    slots.want(second, 40);
    slots.want(third, 4);
    slots.request_wanted();
    for (built, why) in [
        (first, "the heaviest want builds first"),
        (second, "a second build waits behind the first"),
    ] {
        assert_eq!(
            gate.started.recv_timeout(Duration::from_secs(2)),
            Ok(built),
            "{why}"
        );
        assert!(gate.release.send(()).is_ok());
    }
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "a third waits for the next frame"
    );
    slots.begin_frame();
    slots.want(third, 4);
    slots.request_wanted();
    assert_eq!(gate.started.recv_timeout(Duration::from_secs(2)), Ok(third));
    assert!(gate.release.send(()).is_ok());
}

#[test]
fn dropping_the_slots_cancels_the_queued_build() {
    let (builder, gate) = gated();
    let shared = PipelineCompiler::spawn();
    let mut slots = Slots::new(&shared, builder);
    let first = key(BlendMode::SrcOver);
    slots.want(first, 300);
    slots.want(key(BlendMode::DstOut), 40);
    slots.request_wanted();
    assert_eq!(gate.started.recv_timeout(Duration::from_secs(2)), Ok(first));
    drop(slots);
    assert!(gate.release.send(()).is_ok());
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "nothing more starts once the slots are gone"
    );
    assert_eq!(gate.builds.load(Ordering::SeqCst), 1);
}

#[test]
fn a_frames_heaviest_wanted_pipeline_builds_first() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let light = key(BlendMode::DstOut);
    let heavy = key(BlendMode::SrcIn);
    let middle = key(BlendMode::Dst);
    slots.want(light, 40);
    slots.want(heavy, 30_000);
    slots.want(middle, 300);
    slots.want(heavy, 30_000);
    slots.request_wanted();
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(heavy),
        "the draws with the most vertices are built first"
    );
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "one build at a time"
    );
    let _ = gate.release.send(());
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(middle),
        "the next build takes the heavier of what is still wanted"
    );
    let _ = gate.release.send(());
}

#[test]
fn a_heavier_pipeline_wanted_while_another_builds_goes_before_lighter_earlier_ones() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let first = key(BlendMode::DstOut);
    let earlier = key(BlendMode::Dst);
    let heavier = key(BlendMode::SrcIn);
    slots.want(first, 40);
    slots.request_wanted();
    assert_eq!(gate.started.recv_timeout(Duration::from_secs(2)), Ok(first));
    for frame in [
        &[(first, 40), (earlier, 40)][..],
        &[(first, 40), (earlier, 40), (heavier, 30_000)],
    ] {
        slots.begin_frame();
        for &(wanted, vertices) in frame {
            slots.want(wanted, vertices);
        }
        slots.request_wanted();
    }
    let _ = gate.release.send(());
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(heavier),
        "a key wanted earlier but lighter waits for the heavier one"
    );
    let _ = gate.release.send(());
}

#[test]
fn a_frame_that_ended_before_queuing_its_wants_leaves_none_for_the_next() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let stale = key(BlendMode::DstOut);
    let current = key(BlendMode::SrcIn);
    slots.want(stale, 1_000_000);
    slots.begin_frame();
    slots.want(current, 40);
    slots.request_wanted();
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(current),
        "the current frame's want is built"
    );
    let _ = gate.release.send(());
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "the unqueued want of the frame that ended is not built"
    );
}

#[test]
fn a_key_the_latest_frame_no_longer_wants_is_not_built() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let building = key(BlendMode::SrcIn);
    let dropped = key(BlendMode::DstOut);
    slots.want(building, 300);
    slots.want(dropped, 40);
    slots.request_wanted();
    assert_eq!(
        gate.started.recv_timeout(Duration::from_secs(2)),
        Ok(building)
    );
    slots.begin_frame();
    slots.request_wanted();
    let _ = gate.release.send(());
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "the key no frame wants any more stays unbuilt"
    );
    assert_eq!(slots.get(dropped), None);
}
