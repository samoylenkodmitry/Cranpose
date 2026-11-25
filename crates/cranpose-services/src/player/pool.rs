//! Bounded multi-player pool with priority ranking, instant handoff, and automatic audio
//! arbitration for vertical video feeds and multi-player screens.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use cranpose_core::collections::map::HashMap;
use parking_lot::Mutex;

use super::{AudioRole, LoadControl, Player, PlayerBackendFactory, PlayerError};
use crate::media::MediaItem;

/// Resource admission priority of a slot in a [`PlayerPool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlayerPriority {
    /// Background preloading slot (prepared, paused, muted; evicted first under pressure).
    Preload = 0,
    /// Immediate next item in a feed (prepared at frame 0, paused, muted; ready for instant swap).
    Next = 1,
    /// Foreground visible item (active audio arbitration owner, playing).
    Active = 2,
}

/// Configuration for a [`PlayerPool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerPoolConfig {
    /// Maximum number of live [`Player`] instances (and hardware decoders) held simultaneously.
    pub max_concurrent_players: usize,
    /// Buffering profile applied to pooled players (defaults to [`LoadControl::FEED`]).
    pub load_control: LoadControl,
    /// Number of items ahead of the active item to preload in [`PlayerPool::update_feed_window`].
    pub preload_ahead: usize,
    /// Number of items behind the active item to retain warm in [`PlayerPool::update_feed_window`].
    pub preload_behind: usize,
}

impl Default for PlayerPoolConfig {
    fn default() -> Self {
        Self {
            max_concurrent_players: 3,
            load_control: LoadControl::FEED,
            preload_ahead: 1,
            preload_behind: 1,
        }
    }
}

struct PoolSlot {
    player: Player,
    uri: String,
    priority: PlayerPriority,
    last_touch: u64,
}

struct PoolInner {
    config: PlayerPoolConfig,
    factory: Option<Arc<dyn PlayerBackendFactory>>,
    user_muted: AtomicBool,
    clock: AtomicU64,
    slots: Mutex<HashMap<String, PoolSlot>>,
}

/// Bounded multi-player pool managing hardware decoder admission, feed preloading, instant
/// handoff, and single-active-audio arbitration.
#[derive(Clone)]
pub struct PlayerPool {
    inner: Arc<PoolInner>,
}

impl PlayerPool {
    /// Creates a pool using the process-wide platform [`PlayerBackendFactory`].
    pub fn new(config: PlayerPoolConfig) -> Self {
        Self::create(config, None)
    }

    /// Creates a pool using an explicit [`PlayerBackendFactory`].
    pub fn with_backend_factory(
        config: PlayerPoolConfig,
        factory: Arc<dyn PlayerBackendFactory>,
    ) -> Self {
        Self::create(config, Some(factory))
    }

    fn create(
        mut config: PlayerPoolConfig,
        factory: Option<Arc<dyn PlayerBackendFactory>>,
    ) -> Self {
        config.max_concurrent_players = config.max_concurrent_players.max(1);
        Self {
            inner: Arc::new(PoolInner {
                config,
                factory,
                user_muted: AtomicBool::new(false),
                clock: AtomicU64::new(1),
                slots: Mutex::new(HashMap::default()),
            }),
        }
    }

    /// Pool configuration.
    pub fn config(&self) -> PlayerPoolConfig {
        self.inner.config
    }

    /// Number of live [`Player`] instances currently retained in the pool.
    pub fn active_count(&self) -> usize {
        self.inner.slots.lock().len()
    }

    /// Key of the currently [`PlayerPriority::Active`] slot, if any.
    pub fn active_key(&self) -> Option<String> {
        self.inner
            .slots
            .lock()
            .iter()
            .find_map(|(key, slot)| (slot.priority == PlayerPriority::Active).then(|| key.clone()))
    }

    /// Returns the pooled [`Player`] for `key`, if currently warm in the pool.
    pub fn player_for(&self, key: &str) -> Option<Player> {
        self.inner
            .slots
            .lock()
            .get(key)
            .map(|slot| slot.player.clone())
    }

    /// Whether the user has muted feed playback globally across swipes.
    pub fn is_user_muted(&self) -> bool {
        self.inner.user_muted.load(Ordering::Acquire)
    }

    /// Sets global user mute across the feed without disturbing preload silence.
    pub fn set_user_muted(&self, muted: bool) {
        self.inner.user_muted.store(muted, Ordering::Release);
        let active_player = {
            let slots = self.inner.slots.lock();
            slots.values().find_map(|slot| {
                (slot.priority == PlayerPriority::Active).then(|| slot.player.clone())
            })
        };
        if let Some(player) = active_player {
            player.set_muted(muted);
        }
    }

    /// Acquires or promotes a [`Player`] for `key` at `priority`.
    ///
    /// Promoting a slot to [`PlayerPriority::Active`] automatically pauses and mutes any
    /// previously active slot so two feed items never compete for audio output. When at capacity,
    /// the lowest-priority least-recently-used slot is released immediately to hand off its
    /// hardware decoder slot.
    pub fn acquire(
        &self,
        key: impl Into<String>,
        item: MediaItem,
        priority: PlayerPriority,
    ) -> Result<Player, PlayerError> {
        let key = key.into();
        let touch = self.inner.clock.fetch_add(1, Ordering::AcqRel);
        let user_muted = self.is_user_muted();
        let mut demoted: Vec<Player> = Vec::new();
        let mut evicted: Vec<Player> = Vec::new();
        let mut needs_prepare = false;

        let player = {
            let mut slots = self.inner.slots.lock();
            if priority == PlayerPriority::Active {
                for (existing_key, slot) in slots.iter_mut() {
                    if existing_key != &key && slot.priority == PlayerPriority::Active {
                        slot.priority = PlayerPriority::Preload;
                        demoted.push(slot.player.clone());
                    }
                }
            }

            if let Some(slot) = slots.get_mut(&key) {
                slot.priority = priority;
                slot.last_touch = touch;
                if slot.uri != item.uri {
                    slot.uri.clone_from(&item.uri);
                    slot.player.set_media_item(item);
                    needs_prepare = true;
                }
                slot.player.clone()
            } else {
                while slots.len() >= self.inner.config.max_concurrent_players {
                    let victim = slots
                        .iter()
                        .min_by_key(|(_, slot)| (slot.priority, slot.last_touch))
                        .map(|(k, _)| k.clone());
                    match victim {
                        Some(victim_key) => {
                            if let Some(removed) = slots.remove(&victim_key) {
                                evicted.push(removed.player);
                            }
                        }
                        None => break,
                    }
                }
                let role = if priority == PlayerPriority::Active {
                    AudioRole::Primary
                } else {
                    AudioRole::Muted
                };
                let mut builder = Player::builder()
                    .with_load_control(self.inner.config.load_control)
                    .with_audio_role(role);
                if let Some(factory) = &self.inner.factory {
                    builder = builder.with_backend_factory(Arc::clone(factory));
                }
                let player = builder.build()?;
                player.set_media_item(item.clone());
                needs_prepare = true;
                slots.insert(
                    key,
                    PoolSlot {
                        player: player.clone(),
                        uri: item.uri,
                        priority,
                        last_touch: touch,
                    },
                );
                player
            }
        };

        for old in evicted {
            old.release();
        }
        for other in demoted {
            other.pause();
            other.set_muted(true);
        }
        if needs_prepare {
            player.prepare();
        }
        if priority == PlayerPriority::Active {
            player.set_muted(user_muted);
            player.play();
        } else {
            player.pause();
            player.set_muted(true);
        }
        Ok(player)
    }

    /// Synchronizes the pool to a vertical feed window centered on `active_index`: releases every
    /// item outside `[active - preload_behind ..= active + preload_ahead]`, preloads neighbors
    /// muted/paused, and promotes `active_index` to [`PlayerPriority::Active`].
    pub fn update_feed_window(
        &self,
        items: &[(String, MediaItem)],
        active_index: usize,
    ) -> Result<Option<Player>, PlayerError> {
        if items.is_empty() || active_index >= items.len() {
            self.release_all();
            return Ok(None);
        }
        let start = active_index.saturating_sub(self.inner.config.preload_behind);
        let end = (active_index + self.inner.config.preload_ahead).min(items.len() - 1);
        let window = &items[start..=end];

        let removed: Vec<Player> = {
            let mut slots = self.inner.slots.lock();
            let stale: Vec<String> = slots
                .keys()
                .filter(|key| !window.iter().any(|(k, _)| k == *key))
                .cloned()
                .collect();
            stale
                .into_iter()
                .filter_map(|k| slots.remove(&k).map(|s| s.player))
                .collect()
        };
        for player in removed {
            player.release();
        }

        // Promote active item first so it never gets displaced under tight max_concurrent_players.
        let (active_key, active_item) = &items[active_index];
        let active_player = self.acquire(
            active_key.clone(),
            active_item.clone(),
            PlayerPriority::Active,
        )?;

        // Preload next item first (highest feed swipe probability), then other window neighbors.
        let mut capacity_left = self.inner.config.max_concurrent_players.saturating_sub(1);
        if capacity_left > 0 && active_index < end {
            let (k, item) = &items[active_index + 1];
            let _ = self.acquire(k.clone(), item.clone(), PlayerPriority::Next)?;
            capacity_left -= 1;
        }
        for (offset, (k, item)) in window.iter().enumerate() {
            if capacity_left == 0 {
                break;
            }
            let idx = start + offset;
            if idx == active_index || idx == active_index + 1 {
                continue;
            }
            let _ = self.acquire(k.clone(), item.clone(), PlayerPriority::Preload)?;
            capacity_left -= 1;
        }
        Ok(Some(active_player))
    }

    /// Immediately releases and removes the pooled player for `key`, if present.
    pub fn release_key(&self, key: &str) {
        let removed = self.inner.slots.lock().remove(key);
        if let Some(slot) = removed {
            slot.player.release();
        }
    }

    /// Releases every pooled player and clears all slots.
    pub fn release_all(&self) {
        let drained: Vec<PoolSlot> = {
            let mut slots = self.inner.slots.lock();
            slots.drain().map(|(_, slot)| slot).collect()
        };
        for slot in drained {
            slot.player.release();
        }
    }
}
