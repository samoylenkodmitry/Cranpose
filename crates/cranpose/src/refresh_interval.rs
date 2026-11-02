// How often the display a winit window shows on refreshes.

use std::{sync::Arc, time::Duration};

use winit::window::Window;

// A 60 Hz display's interval, for a window whose display says no rate.
pub(crate) const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_nanos(16_666_667);

// The refresh interval of the display `window` shows on.
pub(crate) fn monitor_refresh_interval(window: &Arc<dyn Window>) -> Duration {
    window
        .current_monitor()
        .and_then(|monitor| monitor.current_video_mode())
        .and_then(|mode| mode.refresh_rate_millihertz())
        .map_or(DEFAULT_REFRESH_INTERVAL, |millihertz| {
            Duration::from_nanos(1_000_000_000_000u64 / u64::from(millihertz.get()))
        })
}
