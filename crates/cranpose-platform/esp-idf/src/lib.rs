//! Runs a Cranpose application on an ESP32 display under ESP-IDF.
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
mod touch;

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
    hal::{
        delay::{BLOCK, FreeRtos},
        task::queue::Queue,
    },
    sys::{ESP_ERR_NO_MEM, EspError},
    timer::{EspTaskTimerService, EspTimer},
};
pub use panel::{Backlight, Controller, InitCommand, Lines, Panel, SpiWiring};
pub use small_alloc::SmallBlockAllocator;
pub use touch::{NoTouch, Orientation, St77922Touch, Touch, Xpt2046Calibration, Xpt2046Touch};

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
    /// The rows of a strip of the drawing pass. A strip takes 10 bytes of
    /// internal memory a pixel: 4 for its RGBA drawing and 2 for each of the
    /// two RGB565 buffers the panel reads by DMA in turn. Taller strips cost
    /// fewer draw and transfer calls a picture.
    pub strip_rows: u32,
}

impl DisplayConfig {
    /// The rows of a strip that fit 25 KB of internal memory on a 320-pixel
    /// wide display.
    pub const DEFAULT_STRIP_ROWS: u32 = 8;
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            density: 1.0,
            theme: SystemTheme::Light,
            frame_interval: Duration::from_micros(16_667),
            fonts: &[],
            strip_rows: Self::DEFAULT_STRIP_ROWS,
        }
    }
}

/// The longest the loop runs without blocking. While frames take longer than
/// the frame interval, the loop never waits, and the idle task of its core
/// would not run: ESP-IDF's task watchdog reports it after
/// `CONFIG_ESP_TASK_WDT_TIMEOUT_S`, 5 s by default.
const IDLE_SHARE_INTERVAL: Duration = Duration::from_millis(500);

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

    /// Returns whether an interrupt that called this must yield to the
    /// task it woke.
    fn notify(&self) -> bool {
        // Sequentially consistent on both sides: either the sleeper sees
        // `pending` before it blocks, or this sees `sleeping` and leaves a
        // token for it. A full slot holds one already.
        self.pending.store(true, Ordering::SeqCst);
        self.sleeping.load(Ordering::SeqCst) && self.signal.send_back(0, 0).unwrap_or(false)
    }

    /// Forgets the notices that came so far: the caller handles what they
    /// were for before it waits.
    fn clear(&self) {
        self.pending.store(false, Ordering::SeqCst);
    }

    /// Sleeps until a notice arrives, or returns at once if one came since
    /// the last wait or clear. A token left by a notice that raced the end
    /// of an earlier wait returns one wait early; the loop then waits again.
    /// Returns whether it slept.
    fn wait(&self) -> bool {
        if self.pending.swap(false, Ordering::SeqCst) {
            return false;
        }
        self.sleeping.store(true, Ordering::SeqCst);
        let slept = !self.pending.load(Ordering::SeqCst);
        if slept {
            let _ = self.signal.recv_front(BLOCK);
        }
        self.sleeping.store(false, Ordering::SeqCst);
        self.pending.store(false, Ordering::SeqCst);
        slept
    }
}

/// Composes `content` on `panel` and drives it on the calling task until an
/// error stops it. `touch` delivers the fingers on the panel to `content`;
/// [`NoTouch`] stands for a display without one.
///
/// The calling task becomes the UI thread. Its stack holds composition and
/// layout, so give it at least 32 KB (`CONFIG_ESP_MAIN_TASK_STACK_SIZE` for
/// the main task). An `esp_timer` wakes the task at each frame or timed
/// event, so frame timing does not depend on `CONFIG_FREERTOS_HZ`.
pub fn run(
    panel: Panel<'_>,
    mut touch: impl Touch,
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
    let presenter = Presenter::new(&panel, config.strip_rows)?;
    let wake = Arc::new(Wake::new()?);
    let waker = Arc::clone(&wake);
    framebuffer.set_frame_waker(move || {
        waker.notify();
    });
    // The FreeRTOS tick would round each wait up to whole ticks.
    let timer_wake = Arc::clone(&wake);
    let timers = EspTaskTimerService::new()?;
    let timer = timers.timer(move || {
        timer_wake.notify();
    })?;
    let touched = Arc::new(AtomicBool::new(false));
    let touch_wake = Arc::clone(&wake);
    let touch_event = Arc::clone(&touched);
    touch.arm(move || {
        touch_event.store(true, Ordering::SeqCst);
        touch_wake.notify()
    })?;
    framebuffer.set_active(true);
    let mut screen = Screen {
        framebuffer,
        presenter,
        panel,
        touch: &mut touch,
        config,
        start,
        finger: None,
        stats: None,
        last_tick: None,
    };
    let mut pacer = Pacer {
        wake,
        timer,
        slept: false,
        awake_since: start,
    };
    drive(&mut screen, &mut pacer, &touched)
}

/// The frame loop of [`run`]: `touched` is set when the touch panel's
/// interrupt fires. Without type parameters and out of line, it is one
/// function of this crate in every application, which the IRAM lists of
/// `components/cranpose_hot_code` can name. It borrows the screen, so the
/// code that drops it on an error stays in `run`.
#[inline(never)]
fn drive(
    screen: &mut Screen<'_, '_>,
    pacer: &mut Pacer,
    touched: &AtomicBool,
) -> Result<Infallible, RunError> {
    loop {
        if screen.finger.is_some() || touched.swap(false, Ordering::SeqCst) {
            screen.route_touch();
        }
        let now = Instant::now();
        pacer.begin_round(now);
        screen.tick(now)?;
        // The touch panel's setup goes on between the first pictures.
        let touch_setup = screen.touch.set_up(Instant::now())?;
        // A tick notifies the wake itself when the composition asks for
        // its next frame, which the deadline covers. Clearing before
        // running the posted tasks keeps every later notice for the wait,
        // so the loop does not go round once more for the tick's own one.
        pacer.wake.clear();
        let deadline = match (screen.next_deadline(), touch_setup) {
            (Some(frame), Some(setup)) => Some(frame.min(setup)),
            (frame, setup) => frame.or(setup),
        };
        pacer.sleep(deadline)?;
    }
}

/// The application's side of the loop: its composition, the panel it is
/// drawn on and the touch panel it reads.
struct Screen<'d, 't> {
    framebuffer: Framebuffer,
    presenter: Presenter,
    panel: Panel<'d>,
    touch: &'t mut dyn Touch,
    config: DisplayConfig,
    start: Instant,
    /// Where the finger is while it touches the panel.
    finger: Option<(u16, u16)>,
    stats: Option<FrameStats>,
    /// The start of the last tick. The next one waits `frame_interval` after
    /// it; a wake before then runs the posted tasks only. The waker fires on
    /// every frame request, the composition's own among them.
    last_tick: Option<Instant>,
}

impl Screen<'_, '_> {
    /// Composes a frame and presents what it changes, unless a frame interval
    /// has not passed since the last tick.
    fn tick(&mut self, now: Instant) -> Result<(), EspError> {
        if self
            .last_tick
            .is_some_and(|last| now < last + self.config.frame_interval)
        {
            return Ok(());
        }
        self.last_tick = Some(now);
        let elapsed = now.duration_since(self.start).as_nanos();
        if !self
            .framebuffer
            .tick(u64::try_from(elapsed).unwrap_or(u64::MAX))
        {
            return Ok(());
        }
        let updated = Instant::now();
        self.presenter
            .present(&mut self.framebuffer, &mut self.panel)?;
        let presented = Instant::now();
        let phases = FramePhases {
            update: updated.duration_since(now),
            present: presented.duration_since(updated),
        };
        FrameStats::record_into(&mut self.stats, self.start, phases, presented);
        Ok(())
    }

    /// Runs the tasks posted to the UI thread and returns when the loop must
    /// run next: at the next frame while the composition or a finger on the
    /// panel needs one, else at the composition's next timed event.
    fn next_deadline(&mut self) -> Option<Instant> {
        self.framebuffer.run_pending_tasks();
        let schedule = self.framebuffer.frame_schedule();
        // A finger on the panel is read at every frame: its lifting raises no
        // interrupt that the loop could wait for.
        if self.finger.is_some() || schedule.needs_update || schedule.needs_frame {
            self.last_tick.map(|last| last + self.config.frame_interval)
        } else {
            schedule.next_deadline
        }
    }

    /// Reads the panel and passes what changed since the last read to the
    /// composition: a finger down, moved or lifted. A failed read leaves the
    /// finger where it was. Out of line: it runs only while a finger is on
    /// the panel, and the pointer dispatch it calls is large.
    #[inline(never)]
    fn route_touch(&mut self) {
        let sample = match self.touch.read() {
            Ok(sample) => sample,
            Err(error) => {
                log::warn!("reading the touch panel failed: {error}");
                return;
            }
        };
        let density = self.config.density;
        let point = |(x, y): (u16, u16)| (f32::from(x) / density, f32::from(y) / density);
        let touch = match (self.finger, sample) {
            (None, Some(now)) => Some((TouchPhase::Down, now)),
            (Some(before), Some(now)) if now != before => Some((TouchPhase::Move, now)),
            (Some(before), None) => Some((TouchPhase::Up, before)),
            _ => None,
        };
        if let Some((phase, position)) = touch {
            let (x, y) = point(position);
            self.framebuffer.touch(phase, x, y);
        }
        self.finger = sample;
    }
}

/// Puts the loop to sleep until its deadline or a notice, and lets the idle
/// task run when frames leave no time to sleep.
struct Pacer {
    wake: Arc<Wake>,
    timer: EspTimer<'static>,
    /// Whether the last round slept, which lets the idle task run.
    slept: bool,
    /// When the loop woke from its last sleep.
    awake_since: Instant,
}

impl Pacer {
    /// Notes the start of a round at `now`.
    fn begin_round(&mut self, now: Instant) {
        if self.slept {
            self.awake_since = now;
        }
    }

    /// Sleeps until `deadline` or a notice, or until a notice without a
    /// deadline. A deadline that has passed leaves no sleep, unless the loop
    /// has been awake for [`IDLE_SHARE_INTERVAL`]: then it sleeps one
    /// FreeRTOS tick, the shortest sleep there is.
    fn sleep(&mut self, deadline: Option<Instant>) -> Result<(), EspError> {
        let now = Instant::now();
        let delay = deadline.map(|deadline| deadline.saturating_duration_since(now));
        self.slept = match delay {
            Some(delay) if delay.is_zero() => false,
            delay => {
                if let Some(delay) = delay {
                    self.timer.after(delay)?;
                }
                self.wake.wait()
            }
        };
        if !self.slept && now.duration_since(self.awake_since) >= IDLE_SHARE_INTERVAL {
            FreeRtos::delay_ms(1);
            self.slept = true;
        }
        Ok(())
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
            "{} frames in {:.1} s: update {:.2} ms, draw and send {:.2} ms, slowest {:.1} ms",
            self.frames,
            window.as_secs_f32(),
            average(self.update),
            average(self.present),
            self.slowest.as_secs_f32() * 1000.0,
        );
        *self = Self::new(presented);
    }
}
