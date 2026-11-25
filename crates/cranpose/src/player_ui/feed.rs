//! Compose scoping hook for [`PlayerPool`] in vertical video feeds and multi-player screens.

use cranpose_core::{DisposableEffect, remember};
use cranpose_services::{PlayerPool, PlayerPoolConfig};

/// Creates and remembers a [`PlayerPool`] scoped to this composition, automatically releasing
/// every pooled player and hardware decoder when the feed screen leaves the tree.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberPlayerPool(config: PlayerPoolConfig) -> PlayerPool {
    let pool = remember(|| PlayerPool::new(config)).with(Clone::clone);
    let cleanup = pool.clone();
    DisposableEffect((), move |scope| {
        scope.on_dispose(move || cleanup.release_all())
    });
    pool
}
