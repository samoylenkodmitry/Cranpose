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
mod small_alloc;

use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub use cranpose::framebuffer::{SurfaceError, TouchPhase};
use cranpose::{SystemTheme, framebuffer::Framebuffer};
use esp_idf_svc::{
    hal::{delay::BLOCK, task::queue::Queue},
    sys::{ESP_ERR_NO_MEM, EspError},
    timer::EspTaskTimerService,
};
pub use panel::{Panel, PixelFormat};
pub use small_alloc::SmallBlockAllocator;

#[cfg(feature = "small-block-alloc")]
#[global_allocator]
static ALLOCATOR: SmallBlockAllocator = SmallBlockAllocator;

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

/// Wakes the run loop from any thread. Most notices come from the loop's own
/// thread during a tick, when it does not sleep: they set `pending` and
/// touch no FreeRTOS object. A notice to a sleeping loop leaves a token in
/// `signal`, a FreeRTOS queue of one slot: one call to give it and one to
/// take it, where a pthread mutex and condition variable made four mutex
/// operations and a semaphore for every wait.
struct Wake {
    pending: AtomicBool,
    sleeping: AtomicBool,
    signal: Queue<u8>,
}

impl Wake {
    fn new() -> Result<Self, EspError> {
        let signal = Queue::new(1);
        if signal.as_raw().is_null() {
            return Err(EspError::from_infallible::<ESP_ERR_NO_MEM>());
        }
        Ok(Self {
            pending: AtomicBool::new(false),
            sleeping: AtomicBool::new(false),
            signal,
        })
    }

    fn notify(&self) {
        // Sequentially consistent on both sides: either the sleeper sees
        // `pending` before it blocks, or this sees `sleeping` and leaves a
        // token for it. A full slot holds one already.
        self.pending.store(true, Ordering::SeqCst);
        if self.sleeping.load(Ordering::SeqCst) {
            let _ = self.signal.send_back(0, 0);
        }
    }

    /// Forgets the notices that came so far: the caller handles what they
    /// were for before it waits.
    fn clear(&self) {
        self.pending.store(false, Ordering::SeqCst);
    }

    /// Sleeps until a notice arrives, or returns at once if one came since
    /// the last wait or clear. A token left by a notice that raced the end
    /// of an earlier wait returns one wait early; the loop then waits again.
    fn wait(&self) {
        if self.pending.swap(false, Ordering::SeqCst) {
            return;
        }
        self.sleeping.store(true, Ordering::SeqCst);
        if !self.pending.load(Ordering::SeqCst) {
            let _ = self.signal.recv_front(BLOCK);
        }
        self.sleeping.store(false, Ordering::SeqCst);
        self.pending.store(false, Ordering::SeqCst);
    }
}

/// Composes `content` on `panel` and drives it on the calling task until an
/// error stops it.
///
/// The calling task becomes the UI thread. Its stack holds composition and
/// layout, so give it at least 32 KB (`CONFIG_ESP_MAIN_TASK_STACK_SIZE` for
/// the main task). An `esp_timer` wakes the task at each frame or timed
/// event, so frame timing does not depend on `CONFIG_FREERTOS_HZ`.
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
    let wake = Arc::new(Wake::new()?);
    let waker = Arc::clone(&wake);
    framebuffer.set_frame_waker(move || waker.notify());
    // The FreeRTOS tick would round each wait up to whole ticks.
    let timer_wake = Arc::clone(&wake);
    let timers = EspTaskTimerService::new()?;
    let deadline_timer = timers.timer(move || timer_wake.notify())?;
    framebuffer.set_active(true);
    let mut stats: Option<FrameStats> = None;
    // The start of the last tick. The next one waits `frame_interval` after
    // it; a wake before then runs the posted tasks only. The waker fires on
    // every frame request, the composition's own among them.
    let mut last_tick: Option<Instant> = None;
    loop {
        let now = Instant::now();
        if last_tick.is_none_or(|last| now >= last + config.frame_interval) {
            last_tick = Some(now);
            let elapsed = now.duration_since(start).as_nanos();
            if framebuffer.tick(u64::try_from(elapsed).unwrap_or(u64::MAX)) {
                let updated = Instant::now();
                presenter.present(&mut framebuffer, &mut panel)?;
                let presented = Instant::now();
                let phases = FramePhases {
                    update: updated.duration_since(now),
                    present: presented.duration_since(updated),
                };
                FrameStats::record_into(&mut stats, start, phases, presented);
            }
        }
        // A tick notifies the wake itself when the composition asks for
        // its next frame, which the schedule below covers. Clearing before
        // running the posted tasks keeps every later notice for the wait,
        // so the loop does not go round once more for the tick's own one.
        wake.clear();
        framebuffer.run_pending_tasks();
        let schedule = framebuffer.frame_schedule();
        let deadline = if schedule.needs_update || schedule.needs_frame {
            last_tick.map(|last| last + config.frame_interval)
        } else {
            schedule.next_deadline
        };
        if let Some(deadline) = deadline {
            let delay = deadline.saturating_duration_since(Instant::now());
            if delay.is_zero() {
                continue;
            }
            deadline_timer.after(delay)?;
        }
        wake.wait();
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

    /// Counts a frame into `stats`, which the first frame, logged on its
    /// own, starts.
    fn record_into(
        stats: &mut Option<Self>,
        start: Instant,
        phases: FramePhases,
        presented: Instant,
    ) {
        match stats.as_mut() {
            Some(stats) => stats.record(phases, presented),
            None => {
                log::info!(
                    "first frame after {} ms: update {} ms, draw and send {} ms",
                    presented.duration_since(start).as_millis(),
                    phases.update.as_millis(),
                    phases.present.as_millis(),
                );
                *stats = Some(Self::new(presented));
            }
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
