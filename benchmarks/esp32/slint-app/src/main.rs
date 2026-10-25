// The dashboard of the Cranpose ESP32 benchmarks in Slint 1.18, on the same
// boards, panel driver (../panel) and ESP-IDF settings as the LVGL benchmark.
// It logs its frames in the LVGL benchmark's format.
//
// The ESP32-S3 draws into a frame in PSRAM, as Slint's ESP32-S3 boards do,
// and sends the areas that changed; the ESP32, without PSRAM, draws the
// changed lines into strips as Slint's line renderer hands them out.

mod timeline;

use std::{
    collections::VecDeque,
    ffi::c_void,
    ops::Range,
    ptr::null_mut,
    rc::Rc,
    time::{Duration, Instant},
};

use esp_idf_svc::sys;
use slint::platform::{
    Platform, WindowAdapter,
    software_renderer::{
        LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel, SoftwareRenderer,
    },
};

slint::include_modules!();

unsafe extern "C" {
    fn panel_open(drawn: extern "C" fn(*mut c_void) -> bool, context: *mut c_void);
    fn panel_draw(x1: i32, y1: i32, x2: i32, y2: i32, pixels: *const c_void);
    fn panel_backlight_on();
    fn panel_strip_rows() -> u32;
    fn panel_column_alignment() -> u32;
}

const WIDTH: usize = 320;
const HEIGHT: usize = 480;
const FRAME: Duration = Duration::from_micros(16_667);
const STATS_WINDOW: Duration = Duration::from_secs(5);
/// FreeRTOS's portMAX_DELAY: wait without a timeout.
const BLOCK: sys::TickType_t = sys::TickType_t::MAX;

#[cfg(esp_idf_idf_target_esp32s3)]
const TITLE: &str = "Slint on ESP32-S3";
#[cfg(not(esp_idf_idf_target_esp32s3))]
const TITLE: &str = "Slint on ESP32";

struct Board {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for Board {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

/// The strip buffers the panel reads, in DMA-capable internal RAM. The panel
/// finishes its transfers in the order they were queued, and the semaphore
/// counts the ones it finished.
struct Strips {
    // The bytes sent since the stats window began.
    sent: u64,
    free: Vec<*mut u16>,
    in_flight: VecDeque<*mut u16>,
    done: sys::QueueHandle_t,
    pixels: usize,
}

extern "C" fn drawn(done: *mut c_void) -> bool {
    let mut woken = 0;
    unsafe { sys::xQueueGiveFromISR(done as sys::QueueHandle_t, &mut woken) };
    woken != 0
}

impl Strips {
    fn new(count: usize, pixels: usize) -> Self {
        let done = unsafe { sys::xQueueCreateCountingSemaphore(count as u32, 0) };
        let free = (0..count)
            .map(|_| unsafe {
                sys::heap_caps_malloc(pixels * 2, sys::MALLOC_CAP_DMA | sys::MALLOC_CAP_INTERNAL)
                    as *mut u16
            })
            .collect::<Vec<_>>();
        assert!(free.iter().all(|buffer| !buffer.is_null()), "strip buffers");
        Self {
            sent: 0,
            free,
            in_flight: VecDeque::new(),
            done,
            pixels,
        }
    }

    /// A buffer the panel is not reading, waiting for the oldest transfer
    /// to end when every buffer is out.
    fn take(&mut self) -> &'static mut [u16] {
        if self.free.is_empty() {
            self.wait_for_oldest();
        }
        let buffer = self.free.pop().expect("a buffer came back");
        unsafe { std::slice::from_raw_parts_mut(buffer, self.pixels) }
    }

    fn wait_for_oldest(&mut self) {
        unsafe { sys::xQueueSemaphoreTake(self.done, BLOCK) };
        let buffer = self.in_flight.pop_front().expect("a transfer to wait for");
        self.free.push(buffer);
    }

    /// Queues the transfer of the first pixels of `buffer` to the columns
    /// and rows given.
    fn send(&mut self, columns: Range<usize>, rows: Range<usize>, buffer: &'static mut [u16]) {
        self.in_flight.push_back(buffer.as_mut_ptr());
        self.sent += (columns.len() * rows.len() * 2) as u64;
        unsafe {
            panel_draw(
                columns.start as i32,
                rows.start as i32,
                columns.end as i32 - 1,
                rows.end as i32 - 1,
                buffer.as_ptr().cast(),
            )
        };
    }

    /// Waits until the panel has read every buffer.
    fn finish(&mut self) {
        while !self.in_flight.is_empty() {
            self.wait_for_oldest();
        }
    }
}

/// Draws the changed areas of the window into a frame and sends them, each
/// widened to the columns the panel takes.
fn present_frame(renderer: &SoftwareRenderer, frame: &mut [Rgb565Pixel], strips: &mut Strips) {
    let region = renderer.render(frame, WIDTH);
    let alignment = unsafe { panel_column_alignment() } as usize;
    let rows_per_strip = unsafe { panel_strip_rows() } as usize;
    for (origin, size) in region.iter() {
        let left = origin.x as usize / alignment * alignment;
        let right = (origin.x as usize + size.width as usize).div_ceil(alignment) * alignment;
        let columns = left..right.min(WIDTH);
        let bottom = origin.y as usize + size.height as usize;
        let mut top = origin.y as usize;
        while top < bottom {
            let rows = top..(top + rows_per_strip).min(bottom);
            let strip = strips.take();
            let mut out = strip.iter_mut();
            for row in rows.clone() {
                for pixel in &frame[row * WIDTH + columns.start..row * WIDTH + columns.end] {
                    if let Some(word) = out.next() {
                        *word = pixel.0.swap_bytes();
                    }
                }
            }
            strips.send(columns.clone(), rows.clone(), strip);
            top = rows.end;
        }
    }
    strips.finish();
}

/// A strip that gathers the lines of one range, until it is full or the
/// next line of that range does not follow.
struct OpenStrip {
    columns: Range<usize>,
    rows: Range<usize>,
    pixels: &'static mut [u16],
}

/// Gathers the lines Slint's line renderer hands out into strips, one for
/// each range of columns a line has, and sends each when it is full.
struct LineStrips<'a> {
    strips: &'a mut Strips,
    open: Vec<OpenStrip>,
    rows_per_strip: usize,
}

impl LineStrips<'_> {
    fn flush(&mut self, index: usize) {
        let strip = self.open.swap_remove(index);
        let pixels = strip.columns.len() * strip.rows.len();
        for word in strip.pixels[..pixels].iter_mut() {
            *word = word.swap_bytes();
        }
        self.strips.send(strip.columns, strip.rows, strip.pixels);
    }

    fn finish(mut self) {
        while !self.open.is_empty() {
            self.flush(0);
        }
        self.strips.finish();
    }
}

impl LineBufferProvider for &mut LineStrips<'_> {
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: Range<usize>,
        render_fn: impl FnOnce(&mut [Rgb565Pixel]),
    ) {
        let found = self.open.iter().position(|strip| strip.columns == range);
        let index = match found {
            Some(index) if self.open[index].rows.end == line => index,
            _ => {
                if let Some(index) = found {
                    self.flush(index);
                }
                let pixels = self.strips.take();
                self.open.push(OpenStrip {
                    columns: range.clone(),
                    rows: line..line,
                    pixels,
                });
                self.open.len() - 1
            }
        };
        let strip = &mut self.open[index];
        let start = strip.rows.len() * range.len();
        let words = &mut strip.pixels[start..start + range.len()];
        // Rgb565Pixel is a u16 in a transparent wrapper.
        let line_pixels = unsafe {
            std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<Rgb565Pixel>(), words.len())
        };
        render_fn(line_pixels);
        strip.rows.end = line + 1;
        if strip.rows.len() == self.rows_per_strip {
            self.flush(index);
        }
    }
}

struct Stats {
    window_start: Instant,
    frames: u32,
    busy: Duration,
    slowest: Duration,
}

impl Stats {
    // Counts a frame that took `busy`, ending at `now`; `sent` holds the
    // bytes sent since the window began, which a logged window resets.
    fn record(
        stats: &mut Option<Self>,
        start: Instant,
        busy: Duration,
        now: Instant,
        sent: &mut u64,
    ) {
        let Some(stats) = stats else {
            log::info!(
                "first frame after {} ms: draw and send {} ms",
                now.duration_since(start).as_millis(),
                busy.as_millis()
            );
            *stats = Some(Self {
                window_start: now,
                frames: 0,
                busy: Duration::ZERO,
                slowest: Duration::ZERO,
            });
            *sent = 0;
            return;
        };
        stats.frames += 1;
        stats.busy += busy;
        stats.slowest = stats.slowest.max(busy);
        let window = now.duration_since(stats.window_start);
        if window < STATS_WINDOW {
            return;
        }
        log::info!(
            "{} frames in {:.1} s: busy {:.2} ms, slowest {:.1} ms, sent {:.1} KB a frame",
            stats.frames,
            window.as_secs_f32(),
            stats.busy.as_secs_f32() * 1000.0 / stats.frames as f32,
            stats.slowest.as_secs_f32() * 1000.0,
            *sent as f32 / 1024.0 / stats.frames as f32
        );
        *sent = 0;
        *stats = Self {
            window_start: now,
            frames: 0,
            busy: Duration::ZERO,
            slowest: Duration::ZERO,
        };
    }
}

extern "C" fn wake(task: *mut c_void) {
    unsafe {
        sys::xTaskGenericNotify(
            task as sys::TaskHandle_t,
            0,
            0,
            sys::eNotifyAction_eIncrement,
            null_mut(),
        )
    };
}

fn main() {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let start = Instant::now();
    let rows_per_strip = unsafe { panel_strip_rows() } as usize;
    let buffers = if cfg!(esp_idf_idf_target_esp32s3) {
        2
    } else {
        4
    };
    let mut strips = Strips::new(buffers, WIDTH * rows_per_strip);
    unsafe { panel_open(drawn, strips.done.cast()) };

    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    window.set_size(slint::PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    slint::platform::set_platform(Box::new(Board {
        window: window.clone(),
        start,
    }))
    .expect("the platform is set once");
    let ui = Dashboard::new().expect("the dashboard");
    ui.set_heading(TITLE.into());
    ui.show().expect("the window shows");

    // Only the ESP32-S3 keeps a frame, in PSRAM.
    let mut frame = if cfg!(esp_idf_idf_target_esp32s3) {
        vec![Rgb565Pixel(0); WIDTH * HEIGHT]
    } else {
        Vec::new()
    };
    let task = unsafe { sys::xTaskGetCurrentTaskHandle() };
    let mut timer: sys::esp_timer_handle_t = null_mut();
    let arguments = sys::esp_timer_create_args_t {
        callback: Some(wake),
        arg: task.cast(),
        dispatch_method: sys::esp_timer_dispatch_t_ESP_TIMER_TASK,
        name: c"slint".as_ptr(),
        skip_unhandled_events: true,
    };
    unsafe { sys::esp_timer_create(&arguments, &mut timer) };

    let mut shown = timeline::Shown::default();
    let mut stats = None;
    let mut first_frame = None;
    loop {
        let frame_start = Instant::now();
        let first = *first_frame.get_or_insert(frame_start);
        let t = frame_start.duration_since(first).as_millis() as u64;
        shown.update(&ui, t, ui.get_chart_width(), ui.get_chart_height());
        slint::platform::update_timers_and_animations();
        let drew = window.draw_if_needed(|renderer| {
            if frame.is_empty() {
                let mut lines = LineStrips {
                    strips: &mut strips,
                    open: Vec::new(),
                    rows_per_strip,
                };
                renderer.render_by_line(&mut lines);
                lines.finish();
            } else {
                present_frame(renderer, &mut frame, &mut strips);
            }
        });
        let now = Instant::now();
        if drew {
            let was_first = stats.is_none();
            Stats::record(
                &mut stats,
                start,
                now.duration_since(frame_start),
                now,
                &mut strips.sent,
            );
            if was_first {
                unsafe { panel_backlight_on() };
            }
        }
        let next = frame_start + FRAME;
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            unsafe {
                sys::esp_timer_start_once(timer, wait.as_micros() as u64);
                sys::ulTaskGenericNotifyTake(0, 1, BLOCK);
            }
        }
    }
}
