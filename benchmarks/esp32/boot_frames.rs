use esp_idf_svc::timer::EspTaskTimerService;

const SECONDS: usize = 5;

// Each bucket covers a whole second of the ESP timer from boot. Logging the
// counts after the last second keeps serial writes out of the measurement.
pub(crate) struct BootFrames {
    clock: EspTaskTimerService,
    counts: [u16; SECONDS],
    first_ms: Option<u64>,
    reported: bool,
}

impl BootFrames {
    pub(crate) fn new(clock: EspTaskTimerService) -> Self {
        Self {
            clock,
            counts: [0; SECONDS],
            first_ms: None,
            reported: false,
        }
    }

    pub(crate) fn present(&mut self) {
        if self.reported {
            return;
        }
        let now = self.clock.now();
        if let Some(count) = self.counts.get_mut(now.as_secs() as usize) {
            self.first_ms.get_or_insert(now.as_millis() as u64);
            *count += 1;
        } else {
            self.report();
            self.reported = true;
        }
    }

    // The serial report runs once. Keep its formatting out of the frame loop's IRAM.
    #[cold]
    #[inline(never)]
    fn report(&self) {
        if let Some(ms) = self.first_ms {
            log::info!("BENCH first_ms {ms}");
        }
        for (second, frames) in self.counts.iter().enumerate() {
            log::info!("BENCH second {second} frames {frames}");
        }
    }
}
