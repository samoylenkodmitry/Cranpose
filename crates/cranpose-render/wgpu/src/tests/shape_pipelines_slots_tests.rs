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
    slots.request(drawn);
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
fn demanded_work_is_bounded_and_dropping_the_slots_cancels_what_is_queued() {
    let (builder, gate) = gated();
    let shared = PipelineCompiler::spawn();
    let mut slots = Slots::new(&shared, builder);
    let first = key(BlendMode::SrcOver);
    let second = key(BlendMode::DstOut);
    slots.request(first);
    assert_eq!(gate.started.recv_timeout(Duration::from_secs(2)), Ok(first));
    slots.request(first);
    slots.request(second);
    slots.request(key(BlendMode::Plus));
    assert_eq!(slots.demanded(), &[first, second]);
    drop(slots);
    assert!(gate.release.send(()).is_ok());
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "the queued build never starts"
    );
    assert_eq!(gate.builds.load(Ordering::SeqCst), 1);
}

#[test]
fn a_built_demand_leaves_room_for_the_next() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let first = key(BlendMode::SrcOver);
    let second = key(BlendMode::DstOut);
    let third = key(BlendMode::Plus);
    for _ in 0..2 {
        assert!(gate.release.send(()).is_ok());
    }
    slots.request(first);
    slots.request(second);
    let deadline = Instant::now() + Duration::from_secs(2);
    while slots.get(first).is_none() || slots.get(second).is_none() {
        assert!(
            Instant::now() < deadline,
            "the demanded builds did not finish"
        );
        std::thread::yield_now();
    }
    slots.settle_demanded();
    assert!(slots.demanded().is_empty());
    assert!(gate.release.send(()).is_ok());
    slots.request(third);
    assert_eq!(slots.demanded(), &[third]);
}

#[test]
fn a_frames_heaviest_wanted_pipelines_take_the_demand_slots_first() {
    let (builder, gate) = gated();
    let mut slots = Slots::new(&PipelineCompiler::spawn(), builder);
    let light = key(BlendMode::DstOut);
    let heavy = key(BlendMode::SrcIn);
    let middle = key(BlendMode::Dst);
    slots.want(light, 40);
    slots.want(heavy, 30_000);
    slots.want(middle, 300);
    slots.want(heavy, 30_000);
    slots.settle_demanded();
    for expected in [heavy, middle] {
        assert_eq!(
            gate.started.recv_timeout(Duration::from_secs(2)),
            Ok(expected),
            "the draws with the most vertices are built first"
        );
        let _ = gate.release.send(());
    }
    assert!(
        gate.started
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "a third wanted key waits for a free demand slot"
    );
    assert_eq!(gate.builds.load(Ordering::SeqCst), 2);
}
