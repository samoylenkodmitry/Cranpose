use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

use cranpose_ui_graphics::BlendMode;
use web_time::Duration;

use super::{KeyedBuild, Slots};
use crate::{
    pipeline_compiler::{CompileLane, PipelineCompiler},
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
    let mut slots = Slots::new(&PipelineCompiler::spawn(wgpu::Backend::Vulkan), builder);
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
fn dropping_the_slots_cancels_the_queued_build() {
    let (builder, gate) = gated();
    let shared = PipelineCompiler::spawn(wgpu::Backend::Vulkan);
    let mut slots = Slots::new(&shared, builder);
    let first = key(BlendMode::SrcOver);
    // One thread takes the demanded lane, so the second waits for the first.
    slots.warm_on(first, CompileLane::Demanded);
    slots.warm_on(key(BlendMode::DstOut), CompileLane::Demanded);
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
