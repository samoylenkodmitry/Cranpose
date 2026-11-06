// The `screens-in-turn` feature: the benchmarks' screens one after another
// from one image, each for `SHOWN` and logged as it starts, so a board A/B
// flashes one image a side instead of one a screen. The changing paths, the
// deep moves and the still wall come after the demo's screens except on the
// E32R35T, whose classic ESP32 runs out of memory on the paths, and with the
// `dashboard` feature the dashboard comes last.

use std::time::Duration;

use cranpose::prelude::*;

use crate::demo::{self, Line};

// Long enough for a second of warm-up after the switch and five windows of
// `STATS_WINDOW`.
const SHOWN: Duration = Duration::from_secs(6);

// The frame statistics window of the platform: short, so a screen's time
// holds several.
pub const STATS_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    Demo(Line),
    #[cfg(not(feature = "e32r35t"))]
    Paths,
    #[cfg(not(feature = "e32r35t"))]
    Moves,
    #[cfg(not(feature = "e32r35t"))]
    Wall,
    #[cfg(feature = "dashboard")]
    Dashboard,
}

const SCREENS: &[(&str, Shown)] = &[
    ("demo", Shown::Demo(Line::Seconds)),
    ("drawn", Shown::Demo(Line::Drawn)),
    ("composed", Shown::Demo(Line::Composed)),
    ("workload", Shown::Demo(Line::Workload)),
    #[cfg(not(feature = "e32r35t"))]
    ("paths", Shown::Paths),
    #[cfg(not(feature = "e32r35t"))]
    ("moves", Shown::Moves),
    #[cfg(not(feature = "e32r35t"))]
    ("wall", Shown::Wall),
    #[cfg(feature = "dashboard")]
    ("dashboard", Shown::Dashboard),
];

#[composable]
pub fn Screens() {
    let index = cranpose_core::rememberMutableStateOf(|| 0usize);
    cranpose_core::LaunchedEffect((), async move |_| {
        log::info!("screen {}", SCREENS[0].0);
        loop {
            cranpose_core::delay(SHOWN).await;
            let next = (index.get() + 1) % SCREENS.len();
            log::info!("screen {}", SCREENS[next].0);
            index.set(next);
        }
    });
    match SCREENS[index.get()].1 {
        Shown::Demo(line) => demo::Screen(line),
        #[cfg(not(feature = "e32r35t"))]
        Shown::Paths => crate::changing_paths::ChangingPaths(crate::board::TITLE),
        #[cfg(not(feature = "e32r35t"))]
        Shown::Moves => crate::deep_moves::DeepMoves(crate::board::TITLE),
        #[cfg(not(feature = "e32r35t"))]
        Shown::Wall => crate::still_wall::StillWall(crate::board::TITLE),
        #[cfg(feature = "dashboard")]
        Shown::Dashboard => crate::dashboard::Dashboard(crate::board::TITLE),
    }
}
