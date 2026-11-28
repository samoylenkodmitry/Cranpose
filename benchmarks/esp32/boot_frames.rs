use std::{fmt, time::Duration};

use esp_idf_svc::timer::EspTaskTimerService;

const SECONDS: usize = 5;
// The frame period, the first frame and a line a second.
const LINES: usize = SECONDS + 2;

// Each bucket covers a whole second of the ESP timer from boot. The counts go
// out after the last second, a line whenever the writer takes one, so serial
// writes stay out of the measurement and none is lost.
pub(crate) struct BootFrames {
    clock: EspTaskTimerService,
    frame_interval: Duration,
    counts: [u16; SECONDS],
    first_ms: Option<u64>,
    written: usize,
}

impl BootFrames {
    // `frame_interval` is the app's shortest time from one frame to the next.
    pub(crate) fn new(clock: EspTaskTimerService, frame_interval: Duration) -> Self {
        Self {
            clock,
            frame_interval,
            counts: [0; SECONDS],
            first_ms: None,
            written: 0,
        }
    }

    // Counts a presented frame. After the last second it writes the lines
    // of the report through `write`, which returns whether a line went out.
    pub(crate) fn present(&mut self, write: impl FnMut(fmt::Arguments<'_>) -> bool) {
        if self.written == LINES {
            return;
        }
        let now = self.clock.now();
        if let Some(count) = self.counts.get_mut(now.as_secs() as usize) {
            self.first_ms.get_or_insert(now.as_millis() as u64);
            *count += 1;
        } else {
            self.report(write);
        }
    }

    // The report runs on the frames after the last second. Keep its
    // formatting out of the frame loop's IRAM.
    #[cold]
    #[inline(never)]
    fn report(&mut self, mut write: impl FnMut(fmt::Arguments<'_>) -> bool) {
        let period_us = u32::try_from(self.frame_interval.as_micros()).unwrap_or(u32::MAX);
        while self.written < LINES {
            let took = match self.written {
                0 => write(format_args!("BENCH frame_period_us {period_us}")),
                1 => write(format_args!(
                    "BENCH first_ms {}",
                    self.first_ms.unwrap_or(0)
                )),
                line => write(format_args!(
                    "BENCH second {} frames {}",
                    line - 2,
                    self.counts[line - 2]
                )),
            };
            if !took {
                return;
            }
            self.written += 1;
        }
    }
}
