//! Runs a Cranpose application on an ESP32-S3 display under ESP-IDF.
//!
//! [`run`] composes the application into a [`cranpose::framebuffer::Framebuffer`].
//! For each new picture it draws on the CPU only the areas the picture changes,
//! in strips of a few rows, converts each strip to the panel's RGB565 and sends
//! it to a [`Panel`]. Between frames the task sleeps until the composition's
//! next frame or timed event, or until another thread posts work to it.

#![deny(unsafe_code)]

mod panel;
mod present;

use std::{
    convert::Infallible,
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::{Duration, Instant},
};

pub use cranpose::framebuffer::{SurfaceError, TouchPhase};
use cranpose::{SystemTheme, framebuffer::Framebuffer};
use esp_idf_svc::sys::EspError;
pub use panel::{Panel, PixelFormat};

use crate::present::Presenter;

/// A failure that stops [`run`].
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The panel size or the framebuffer memory is unusable.
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    /// ESP-IDF refused a panel operation.
    #[error(transparent)]
    Esp(#[from] EspError),
}

/// How [`run`] shows the application.
#[derive(Clone, Copy, Debug)]
pub struct DisplayConfig {
    /// Physical pixels per point.
    pub density: f32,
    /// The system theme the content reads.
    pub theme: SystemTheme,
    /// The shortest time from one frame to the next.
    pub frame_interval: Duration,
    /// The fonts text draws in. With none, text draws in the embedded default
    /// font, which the `embedded-default-font` feature of `cranpose` adds:
    /// 1.3 MB of flash.
    pub fonts: &'static [&'static [u8]],
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            density: 1.0,
            theme: SystemTheme::Light,
            frame_interval: Duration::from_micros(16_667),
            fonts: &[],
        }
    }
}

#[derive(Default)]
struct Wake {
    pending: Mutex<bool>,
    signal: Condvar,
}

impl Wake {
    fn notify(&self) {
        *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.signal.notify_one();
    }

    fn wait_until(&self, deadline: Option<Instant>) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        while !*pending {
            match deadline {
                None => {
                    pending = self
                        .signal
                        .wait(pending)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                Some(deadline) => {
                    let Some(timeout) = deadline.checked_duration_since(Instant::now()) else {
                        break;
                    };
                    pending = self
                        .signal
                        .wait_timeout(pending, timeout)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
        *pending = false;
    }
}

/// Composes `content` on `panel` and drives it on the calling task until an
/// error stops it.
///
/// The calling task becomes the UI thread. Its stack holds composition and
/// layout, so give it at least 32 KB (`CONFIG_ESP_MAIN_TASK_STACK_SIZE` for
/// the main task). Timed waits round to the FreeRTOS tick: set
/// `CONFIG_FREERTOS_HZ=1000` to keep `frame_interval`.
pub fn run(
    mut panel: Panel,
    config: DisplayConfig,
    content: impl FnMut() + 'static,
) -> Result<Infallible, RunError> {
    let start = Instant::now();
    let mut framebuffer = Framebuffer::new(
        panel.width(),
        panel.height(),
        config.density,
        config.theme,
        config.fonts,
        content,
    )?;
    let mut presenter = Presenter::new(&panel)?;
    let wake = Arc::new(Wake::default());
    let waker = Arc::clone(&wake);
    framebuffer.set_frame_waker(move || waker.notify());
    framebuffer.set_active(true);
    let mut stats: Option<FrameStats> = None;
    loop {
        let frame_start = Instant::now();
        framebuffer.run_pending_tasks();
        let elapsed = frame_start.duration_since(start).as_nanos();
        if framebuffer.tick(u64::try_from(elapsed).unwrap_or(u64::MAX)) {
            let updated = Instant::now();
            presenter.present(&mut framebuffer, &mut panel)?;
            let presented = Instant::now();
            let phases = FramePhases {
                update: updated.duration_since(frame_start),
                present: presented.duration_since(updated),
            };
            match stats.as_mut() {
                Some(stats) => stats.record(phases, presented),
                None => {
                    log::info!(
                        "first frame after {} ms: update {} ms, draw and send {} ms",
                        presented.duration_since(start).as_millis(),
                        phases.update.as_millis(),
                        phases.present.as_millis(),
                    );
                    stats = Some(FrameStats::new(presented));
                }
            }
        }
        let schedule = framebuffer.frame_schedule();
        let deadline = if schedule.needs_update || schedule.needs_frame {
            Some(frame_start + config.frame_interval)
        } else {
            schedule.next_deadline
        };
        wake.wait_until(deadline);
    }
}

const STATS_WINDOW: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct FramePhases {
    update: Duration,
    present: Duration,
}

struct FrameStats {
    window_start: Instant,
    frames: u32,
    update: Duration,
    present: Duration,
    slowest: Duration,
}

impl FrameStats {
    fn new(window_start: Instant) -> Self {
        Self {
            window_start,
            frames: 0,
            update: Duration::ZERO,
            present: Duration::ZERO,
            slowest: Duration::ZERO,
        }
    }

    fn record(&mut self, phases: FramePhases, presented: Instant) {
        self.frames += 1;
        self.update += phases.update;
        self.present += phases.present;
        self.slowest = self.slowest.max(phases.update + phases.present);
        let window = presented.duration_since(self.window_start);
        if window < STATS_WINDOW {
            return;
        }
        let average = |total: Duration| total.as_secs_f32() * 1000.0 / self.frames as f32;
        log::info!(
            "{} frames in {:.1} s: update {:.1} ms, draw and send {:.1} ms, slowest {:.1} ms",
            self.frames,
            window.as_secs_f32(),
            average(self.update),
            average(self.present),
            self.slowest.as_secs_f32() * 1000.0,
        );
        *self = Self::new(presented);
    }
}
