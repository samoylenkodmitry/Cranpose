// Lazy list items composed ahead while a frame loop waits.

use std::time::{Duration, Instant};

use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;

// How much earlier than the next frame or timed event the passes stop, as on
// Android.
const IDLE_PREFETCH_MARGIN: Duration = Duration::from_millis(1);

// Composes lazy list items that frames left beyond their viewports
// (`AppShell::run_idle_prefetch`) until a margin before `until`, the start
// of the next frame or timed event: none when that leaves no room. Whether
// passes ran and items are left, so the loop looks again at once; while no
// pass fits, it waits as it would have.
pub(crate) fn prefetch_until<R>(app: &mut AppShell<R>, now: Instant, until: Instant) -> bool
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    until
        .checked_sub(IDLE_PREFETCH_MARGIN)
        .filter(|deadline| *deadline > now)
        .is_some_and(|deadline| app.run_idle_prefetch(Some(deadline), |_| false))
        && app.has_idle_prefetch_work()
}
