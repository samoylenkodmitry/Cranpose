#![allow(clippy::print_stdout, reason = "a memory test prints what it measured")]

use std::{
    alloc::System,
    cell::Cell,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use cranpose::{
    LoadControl, MediaItem, MediaProperties, Player, PlayerBackendFactory, PlayerPool,
    PlayerPoolConfig, PlayerState, composable, rememberMuteButtonState,
    rememberPlayPauseButtonState, rememberPresentationState, rememberProgressStateWithTickInterval,
};
use cranpose_core::{Composition, MemoryApplier, location_key};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "../player_test_support.rs"]
mod player_test_support;

use player_test_support::TestFactory;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const IDLE_FRAMES: usize = 120;
const SWIPE_WARMUP: usize = 20;
const SWIPE_TOTAL: usize = 120;
const SWIPE_HEAP_BOUND: isize = 32 * 1024;

#[composable]
fn PausedControls(player: Player, recompositions: Rc<Cell<u32>>) {
    recompositions.set(recompositions.get() + 1);
    let _pp = rememberPlayPauseButtonState(&player);
    let _mute = rememberMuteButtonState(&player);
    let _progress = rememberProgressStateWithTickInterval(&player, Duration::from_millis(50));
    let _pres = rememberPresentationState(&player);
}

fn verify_paused_steady_frame_zero_allocations() {
    let factory = TestFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player");
    let sink = factory.sink();
    player.set_media_item(MediaItem::new("https://example.invalid/paused.mp4"));
    player.prepare();
    sink.set_media_properties(MediaProperties {
        duration: Some(Duration::from_secs(60)),
        is_live: false,
        is_seekable: true,
    });
    sink.set_playback_state(PlayerState::Ready);

    let mut composition = Composition::new(MemoryApplier::new());
    let recompositions = Rc::new(Cell::new(0_u32));
    {
        let (p, r) = (player.clone(), Rc::clone(&recompositions));
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                PausedControls(p.clone(), Rc::clone(&r));
            })
            .expect("initial render");
    }
    let runtime = composition.runtime_handle();
    for _ in 0..8 {
        runtime.drain_ui();
        composition.process_invalid_scopes().expect("settle");
    }
    let before_recomps = recompositions.get();

    let region = Region::new(GLOBAL);
    for _ in 0..IDLE_FRAMES {
        runtime.drain_ui();
        composition.process_invalid_scopes().expect("idle tick");
    }
    let change = region.change();
    println!(
        "paused steady frames ({IDLE_FRAMES}): allocations={}, bytes_allocated={}, recompositions={}",
        change.allocations,
        change.bytes_allocated,
        recompositions.get() - before_recomps
    );
    assert_eq!(
        recompositions.get(),
        before_recomps,
        "paused player must trigger zero recompositions"
    );
    assert_eq!(
        change.allocations, 0,
        "paused steady frames must allocate zero framework objects"
    );
    assert_eq!(
        change.bytes_allocated, 0,
        "paused steady frames must allocate zero bytes"
    );
    player.release();
}

fn verify_100_swipe_feed_bounded_memory_and_decoders() {
    let factory = TestFactory::new();
    let pool = PlayerPool::with_backend_factory(
        PlayerPoolConfig {
            max_concurrent_players: 3,
            load_control: LoadControl::FEED,
            preload_ahead: 1,
            preload_behind: 1,
        },
        Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>,
    );
    let feed: Vec<(String, MediaItem)> = (0..SWIPE_TOTAL + 4)
        .map(|i| {
            (
                format!("clip-{i}"),
                MediaItem::new(format!("https://example.invalid/feed/{i}.mp4")),
            )
        })
        .collect();

    for idx in 0..SWIPE_WARMUP {
        drop(pool.update_feed_window(&feed, idx).expect("warmup swipe"));
    }
    let region = Region::new(GLOBAL);
    for idx in SWIPE_WARMUP..SWIPE_TOTAL {
        let active = pool
            .update_feed_window(&feed, idx)
            .expect("measured swipe")
            .expect("active player");
        assert!(active.play_when_ready(), "active feed item must auto-play");
        assert!(!active.is_muted(), "active feed item must be unmuted");
        assert!(
            pool.active_count() <= 3,
            "pooled players must stay within configured cap"
        );
        assert!(
            factory.live.load(Ordering::Acquire) <= 3,
            "live decoder backends must stay within configured cap"
        );
    }
    let change = region.change();
    let net_bytes = change.bytes_allocated as isize - change.bytes_deallocated as isize;
    let peak = factory.peak.load(Ordering::Acquire);
    println!(
        "100-swipe feed (swipes {SWIPE_WARMUP}..{SWIPE_TOTAL}): peak_decoders={peak}, net_heap_bytes={net_bytes}"
    );
    assert!(peak <= 3, "peak live decoders {peak} exceeded bound 3");
    assert!(
        net_bytes < SWIPE_HEAP_BOUND,
        "100-swipe net retained heap growth {net_bytes} bytes exceeded {SWIPE_HEAP_BOUND}"
    );

    pool.release_all();
    assert_eq!(pool.active_count(), 0, "release_all must clear pool slots");
    assert_eq!(
        factory.live.load(Ordering::Acquire),
        0,
        "pool release must free every backend"
    );
}

fn main() {
    verify_paused_steady_frame_zero_allocations();
    verify_100_swipe_feed_bounded_memory_and_decoders();
}
