//! What the streamed scenes cost the headset, measured when the `stream-costs` feature is on.

#[cfg(feature = "stream-costs")]
pub(crate) use measured::Costs;
#[cfg(not(feature = "stream-costs"))]
pub(crate) use unmeasured::Costs;

use super::external_texture::ExternalTexture;

#[cfg(not(feature = "stream-costs"))]
mod unmeasured {
    use super::ExternalTexture;

    // No measurement: the passes run with no timing.
    #[derive(Default)]
    pub(crate) struct Costs;

    impl Costs {
        pub(crate) fn new(_importer: &mut ExternalTexture) -> Self {
            Self
        }

        pub(crate) fn begin(&mut self, _importer: &mut ExternalTexture) {}

        pub(crate) fn end(&mut self, _importer: &mut ExternalTexture) {}
    }
}

#[cfg(feature = "stream-costs")]
mod measured {
    use std::time::{Duration, Instant};

    use super::ExternalTexture;

    // How often the log reports the costs.
    const REPORT_EVERY: Duration = Duration::from_secs(1);

    // What the passes cost since the last report: the CPU time of each frame's pass and its
    // GPU time from a timer query.
    #[derive(Default)]
    pub(crate) struct Costs {
        frames: u32,
        cpu: Duration,
        gpu: Duration,
        gpu_frames: u32,
        started: Option<Instant>,
        since: Option<Instant>,
    }

    impl Costs {
        pub(crate) fn new(importer: &mut ExternalTexture) -> Self {
            importer.enable_timing();
            Self::default()
        }

        pub(crate) fn begin(&mut self, importer: &mut ExternalTexture) {
            importer.begin_timer();
            self.started = Some(Instant::now());
        }

        // Counts a drawn frame, and logs the costs once in a while.
        pub(crate) fn end(&mut self, importer: &mut ExternalTexture) {
            importer.end_timer();
            self.frames += 1;
            self.cpu += self
                .started
                .take()
                .map_or(Duration::ZERO, |at| at.elapsed());
            while let Some(gpu) = importer.gpu_time() {
                self.gpu += gpu;
                self.gpu_frames += 1;
            }
            let since = *self.since.get_or_insert_with(Instant::now);
            if since.elapsed() < REPORT_EVERY {
                return;
            }
            let mean = |total: Duration, count: u32| {
                total.as_secs_f64() * 1000.0 / f64::from(count.max(1))
            };
            log::info!(
                "cranpose xr: streamed scene drew {} frames, {:.2} ms of CPU and {:.2} ms of GPU for each",
                self.frames,
                mean(self.cpu, self.frames),
                mean(self.gpu, self.gpu_frames),
            );
            *self = Self {
                since: Some(Instant::now()),
                ..Self::default()
            };
        }
    }
}
