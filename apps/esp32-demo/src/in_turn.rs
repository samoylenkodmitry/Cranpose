// The `screens-in-turn` feature: the benchmarks' screens one after another
// from one image, each for `SHOWN` and logged as it starts, so a board A/B
// flashes one image a side instead of one a screen. With the `dashboard`
// feature the dashboard comes last.

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
    #[cfg(feature = "dashboard")]
    Dashboard,
}

const SCREENS: &[(&str, Shown)] = &[
    ("demo", Shown::Demo(Line::Seconds)),
    ("drawn", Shown::Demo(Line::Drawn)),
    ("composed", Shown::Demo(Line::Composed)),
    ("workload", Shown::Demo(Line::Workload)),
    #[cfg(feature = "dashboard")]
    ("dashboard", Shown::Dashboard),
];

#[composable]
pub fn Screens() {
    let index = cranpose_core::rememberMutableStateOf(|| 0usize);
    cranpose_core::LaunchedEffectAsync((), move |_| {
        std::boxed::Box::pin(async move {
            log::info!("screen {}", SCREENS[0].0);
            loop {
                cranpose_core::delay(SHOWN).await;
                let next = (index.get() + 1) % SCREENS.len();
                log::info!("screen {}", SCREENS[next].0);
                index.set(next);
            }
        })
    });
    match SCREENS[index.get()].1 {
        Shown::Demo(line) => demo::Screen(line),
        #[cfg(feature = "dashboard")]
        Shown::Dashboard => crate::dashboard::Dashboard(crate::board::TITLE),
    }
}
