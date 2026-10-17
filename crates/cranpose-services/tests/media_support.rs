use std::sync::{Mutex, MutexGuard, PoisonError};

/// Every test that installs, clears or drives the global media player holds
/// this guard, so no other test swaps the player while it runs.
pub(crate) fn one_media_player_at_a_time() -> MutexGuard<'static, ()> {
    static MEDIA_PLAYER: Mutex<()> = Mutex::new(());
    MEDIA_PLAYER.lock().unwrap_or_else(PoisonError::into_inner)
}
