use cranpose_foundation::PointerEvent;
use cranpose_render_common::Renderer;
use web_time::Instant;

use crate::{AppShell, ShellApp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Platform and animation-clock timestamps for one pointer sample.
pub struct PointerEventTime {
    /// Timestamp supplied by the platform, in its millisecond clock domain.
    pub platform_time_ms: Option<i64>,
    /// Timestamp in the animation frame-clock domain.
    pub animation_time_nanos: u64,
    /// The instant at which the shell received the sample.
    pub received_at: Instant,
}

impl PointerEventTime {
    pub(crate) fn stamp(self, event: PointerEvent) -> PointerEvent {
        event
            .with_time_ms(self.platform_time_ms)
            .with_animation_time_nanos(self.animation_time_nanos)
            .with_received_at(self.received_at)
    }
}

impl ShellApp {
    pub(crate) fn realtime_pointer_event_time(
        &self,
        platform_time_ms: Option<i64>,
    ) -> PointerEventTime {
        let received_at = Instant::now();
        PointerEventTime {
            platform_time_ms,
            animation_time_nanos: self
                .frame_time_nanos_at(received_at)
                .max(self.last_frame_time_nanos),
            received_at,
        }
    }
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    /// Timestamp a live input sample against the current animation clock.
    pub fn realtime_pointer_event_time(&self, platform_time_ms: Option<i64>) -> PointerEventTime {
        self.app.realtime_pointer_event_time(platform_time_ms)
    }

    /// Timestamp deterministic input at the most recently processed frame. The sample counts as
    /// received at the time of the call.
    pub fn exact_pointer_event_time(&self, platform_time_ms: Option<i64>) -> PointerEventTime {
        PointerEventTime {
            platform_time_ms,
            animation_time_nanos: self.app.last_frame_time_nanos,
            received_at: Instant::now(),
        }
    }
}
