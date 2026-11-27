//! Shared event-driven synchronization primitive for Media3 Compose state holders.

use cranpose_core::{CollectEvents, rememberEventStream, rememberKeyed};
use cranpose_services::{Player, PlayerEvent};

#[track_caller]
pub(super) fn remember_player_holder<H, S>(
    player: &Player,
    init: impl FnOnce(Player) -> H,
    extract: impl Fn(&Player, &PlayerEvent) -> Option<S> + Send + Sync + 'static,
    mut apply: impl FnMut(&H, S) + 'static,
) -> H
where
    H: Clone + 'static,
    S: Send + 'static,
{
    let id = player.id();
    let holder = rememberKeyed(id, |_| init(player.clone()));
    let handle = player.clone();
    let updates = rememberEventStream(id, move |sender| {
        let p = handle.clone();
        handle.observe_events(move |event| {
            if let Some(snapshot) = extract(&p, &event) {
                sender.send(snapshot);
            }
        })
    });
    let target = holder.clone();
    CollectEvents(updates, id, move |snapshot| apply(&target, snapshot));
    holder
}
