use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose_services::{
    AudioRole, LoadControl, MediaCache, MediaCacheError, MediaItem, MediaMetadata,
    PlaybackParameters, PlayerBackend, PlayerBackendFactory, PlayerError, PlayerPool,
    PlayerPoolConfig, PlayerPriority, PlayerSink, TrackSelectionParameters, path_from_uri,
};

struct CountingBackend {
    live: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
}

impl CountingBackend {
    fn create(
        live: Arc<AtomicUsize>,
        peak: &AtomicUsize,
        opens: Arc<AtomicUsize>,
        releases: Arc<AtomicUsize>,
    ) -> Self {
        let now = live.fetch_add(1, Ordering::AcqRel) + 1;
        peak.fetch_max(now, Ordering::AcqRel);
        Self {
            live,
            opens,
            releases,
        }
    }
}

impl PlayerBackend for CountingBackend {
    fn open(&self, _item: &MediaItem, _start_position: Duration) {
        self.opens.fetch_add(1, Ordering::AcqRel);
    }
    fn play(&self) {}
    fn pause(&self) {}
    fn stop(&self) {}
    fn seek_to(&self, _position: Duration) {}
    fn set_volume(&self, _volume: f32) {}
    fn set_playback_parameters(&self, _parameters: PlaybackParameters) {}
    fn set_track_selection(&self, _parameters: &TrackSelectionParameters) {}
    fn release(&self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
        self.releases.fetch_add(1, Ordering::AcqRel);
    }
}

struct CountingFactory {
    live: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    opens: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
}

impl CountingFactory {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            live: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            opens: Arc::new(AtomicUsize::new(0)),
            releases: Arc::new(AtomicUsize::new(0)),
        })
    }
}

impl PlayerBackendFactory for CountingFactory {
    fn create(
        &self,
        _sink: PlayerSink,
        _load_control: LoadControl,
        _audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        Ok(Arc::new(CountingBackend::create(
            Arc::clone(&self.live),
            &self.peak,
            Arc::clone(&self.opens),
            Arc::clone(&self.releases),
        )))
    }
}

#[test]
fn player_pool_100_swipe_feed_bounds_decoders_hands_off_and_arbitrates_audio() {
    let factory = CountingFactory::new();
    let config = PlayerPoolConfig {
        max_concurrent_players: 3,
        load_control: LoadControl::FEED,
        preload_ahead: 1,
        preload_behind: 1,
    };
    let pool = PlayerPool::with_backend_factory(
        config,
        Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>,
    );

    let feed: Vec<(String, MediaItem)> = (0..105)
        .map(|i| {
            (
                format!("clip-{i}"),
                MediaItem::new(format!("https://cdn.example.com/feed/{i}.mp4")),
            )
        })
        .collect();

    // First item activates clip-0 and preloads clip-1 muted.
    let p0 = pool
        .update_feed_window(&feed, 0)
        .expect("window 0")
        .expect("active 0");
    assert_eq!(pool.active_key().as_deref(), Some("clip-0"));
    assert!(p0.play_when_ready());
    assert!(!p0.is_muted());
    let preloaded_1 = pool.player_for("clip-1").expect("clip-1 preloaded");
    assert!(!preloaded_1.play_when_ready());
    assert!(preloaded_1.is_muted());
    assert_eq!(
        preloaded_1.audio_role(),
        AudioRole::Primary,
        "preloaded feed slots must keep Primary audio role so promoting them can unmute and drive OS controls"
    );

    // Swiping to clip-1 reuses the preloaded player with zero extra open (instant handoff).
    let opens_before_swipe = factory.opens.load(Ordering::Acquire);
    let p1 = pool
        .update_feed_window(&feed, 1)
        .expect("window 1")
        .expect("active 1");
    assert_eq!(p1.id(), preloaded_1.id());
    assert!(p1.play_when_ready());
    assert!(!p1.is_muted());
    assert!(!p0.play_when_ready());
    assert!(p0.is_muted());
    // Only clip-2 was newly opened; clip-1 was handed off warm.
    assert_eq!(
        factory.opens.load(Ordering::Acquire),
        opens_before_swipe + 1
    );

    // Run 100 consecutive swipes: live backends never exceed max_concurrent_players (3).
    for idx in 2..102 {
        let active = pool
            .update_feed_window(&feed, idx)
            .expect("swipe succeeds")
            .expect("active player");
        assert!(active.play_when_ready());
        assert!(!active.is_muted());
        assert!(pool.active_count() <= 3);
        assert!(factory.live.load(Ordering::Acquire) <= 3);
    }
    assert!(factory.peak.load(Ordering::Acquire) <= 3);

    // Global user mute stays muted across subsequent swipes.
    pool.set_user_muted(true);
    let p102 = pool
        .update_feed_window(&feed, 102)
        .expect("window 102")
        .expect("active 102");
    assert!(p102.is_muted());
    assert!(pool.is_user_muted());

    // Releasing the pool frees every single backend with zero leaks.
    pool.release_all();
    assert_eq!(pool.active_count(), 0);
    assert_eq!(factory.live.load(Ordering::Acquire), 0);
}

#[test]
fn player_pool_priority_eviction_never_drops_active_before_preload() {
    let factory = CountingFactory::new();
    let config = PlayerPoolConfig {
        max_concurrent_players: 2,
        ..PlayerPoolConfig::default()
    };
    let pool = PlayerPool::with_backend_factory(
        config,
        Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>,
    );

    let _active = pool
        .acquire(
            "a",
            MediaItem::new("https://example.com/a.mp4"),
            PlayerPriority::Active,
        )
        .expect("active");
    let _preload = pool
        .acquire(
            "b",
            MediaItem::new("https://example.com/b.mp4"),
            PlayerPriority::Preload,
        )
        .expect("preload");
    let _next = pool
        .acquire(
            "c",
            MediaItem::new("https://example.com/c.mp4"),
            PlayerPriority::Next,
        )
        .expect("next");

    assert!(pool.player_for("a").is_some(), "active kept");
    assert!(pool.player_for("c").is_some(), "next kept");
    assert!(
        pool.player_for("b").is_none(),
        "lowest-priority preload evicted"
    );
    pool.release_all();
    assert_eq!(factory.live.load(Ordering::Acquire), 0);
}

#[test]
fn media_cache_persists_evicts_lru_and_resolves_offline_items() {
    let dir = tempfile::tempdir().expect("temp cache dir");
    let cache = MediaCache::open(dir.path(), 10).expect("open cache");

    assert_eq!(
        cache.put_bytes("https://example.com/huge.mp4", &[0_u8; 11]),
        Err(MediaCacheError::EntryTooLarge {
            size: 11,
            max_bytes: 10
        })
    );

    cache
        .put_bytes("https://example.com/a.mp4", b"12345")
        .expect("put a");
    cache
        .put_bytes("https://example.com/b.mp4", b"67890")
        .expect("put b");
    assert_eq!(cache.stats().total_bytes, 10);

    // Touch 'a' via resolve_media_item so 'b' becomes least-recently-used.
    let item_a = MediaItem::new("https://example.com/a.mp4")
        .with_mime_type("video/mp4")
        .with_metadata(MediaMetadata::titled("Clip A"));
    let resolved_a = cache.resolve_media_item(&item_a);
    assert!(resolved_a.uri.starts_with("file://"));
    assert_eq!(resolved_a.metadata.title, "Clip A");
    assert_eq!(resolved_a.mime_type.as_deref(), Some("video/mp4"));
    let path_a = path_from_uri(&resolved_a.uri).expect("local path");
    assert_eq!(std::fs::read(&path_a).expect("read a"), b"12345");

    // Inserting 'c' (4 bytes) must evict LRU 'b' and keep 'a'.
    cache
        .put_bytes("https://example.com/c.mp4", b"abcd")
        .expect("put c");
    assert!(cache.contains("https://example.com/a.mp4"));
    assert!(cache.contains("https://example.com/c.mp4"));
    assert!(!cache.contains("https://example.com/b.mp4"));
    assert_eq!(cache.stats().evictions, 1);
    assert_eq!(cache.stats().total_bytes, 9);

    // Reopening from the same directory preserves persisted entries across app restarts.
    drop(cache);
    let reopened = MediaCache::open(dir.path(), 10).expect("reopen cache");
    assert!(reopened.contains("https://example.com/a.mp4"));
    assert!(reopened.contains("https://example.com/c.mp4"));
    assert_eq!(reopened.stats().total_bytes, 9);

    reopened.clear();
    assert_eq!(reopened.stats().entry_count, 0);
    assert_eq!(reopened.stats().total_bytes, 0);
}
