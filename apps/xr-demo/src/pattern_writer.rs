// Writes a moving test pattern into an external surface from its own thread, as a video decoder
// writes its pictures. A bar crosses the picture and a colored square marks each corner, so a
// screenshot shows whether the picture is whole, flipped or mirrored. A split picture tints its
// first half red and its second half blue, for a stereo surface.
#![cfg_attr(
    target_os = "android",
    expect(
        unsafe_code,
        reason = "the window of an external surface is wrapped from its pointer"
    )
)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use cranpose_xr::ExternalSurface;

// How the picture divides into tinted halves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Split {
    Whole,
    // The left half is red and the right half blue.
    LeftRight,
    // The top half is red and the bottom half blue.
    TopBottom,
}

// The writer of one surface. It stops when dropped. The thread ends at its next frame, so the
// drop never waits for the compositor.
pub(crate) struct PatternWriter {
    stop: Arc<AtomicBool>,
}

impl PatternWriter {
    pub(crate) fn start(surface: ExternalSurface, split: Split) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("pattern-writer".into())
            .spawn(move || paint(surface, split, &flag));
        if let Err(error) = spawned {
            log::error!("xr demo: no pattern thread: {error}");
        }
        Self { stop }
    }
}

impl Drop for PatternWriter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(target_os = "android"))]
fn paint(_surface: ExternalSurface, _split: Split, _stop: &AtomicBool) {
    // Only a headset makes surfaces, so no thread starts here.
}

#[cfg(target_os = "android")]
fn paint(surface: ExternalSurface, split: Split, stop: &AtomicBool) {
    use std::{
        mem::MaybeUninit,
        ptr::NonNull,
        time::{Duration, Instant},
    };

    use ndk::{hardware_buffer_format::HardwareBufferFormat, native_window::NativeWindow};

    const FRAME: Duration = Duration::from_micros(16_667);

    let Some(pointer) = NonNull::new(surface.as_ptr().cast()) else {
        return;
    };
    // SAFETY: the pointer is the native window of `surface`, which lives through this call. The
    // clone takes a reference of its own.
    let window = unsafe { NativeWindow::clone_from_ptr(pointer) };
    drop(surface);
    // The surface starts with the size that the headset gave it
    let (width, height) = (window.width().max(2), window.height().max(2));
    if let Err(error) =
        window.set_buffers_geometry(width, height, Some(HardwareBufferFormat::R8G8B8A8_UNORM))
    {
        log::error!("xr demo: the surface refuses RGBA: {error}");
        return;
    }
    let mut frame = 0_u32;
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        // Locking waits for a free buffer, so the compositor paces the writer
        let mut buffer = match window.lock(None) {
            Ok(buffer) => buffer,
            Err(error) => {
                log::info!("xr demo: the pattern ends: {error}");
                return;
            }
        };
        let (width, height) = (buffer.width() as u32, buffer.height() as u32);
        let Some(rows) = buffer.lines() else {
            log::error!("xr demo: the surface is not a 4 byte format");
            return;
        };
        for (y, row) in rows.enumerate() {
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let rgba = pixel_at(x as u32, y as u32, (width, height), frame, split);
                for (byte, value) in pixel.iter_mut().zip(rgba) {
                    *byte = MaybeUninit::new(value);
                }
            }
        }
        // Dropping the guard posts the buffer
        drop(buffer);
        frame = frame.wrapping_add(1);
        next += FRAME;
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

#[cfg(target_os = "android")]
mod colors {
    pub(super) const RED: [u8; 4] = [230, 40, 40, 255];
    pub(super) const GREEN: [u8; 4] = [40, 220, 70, 255];
    pub(super) const BLUE: [u8; 4] = [50, 90, 240, 255];
    pub(super) const YELLOW: [u8; 4] = [240, 220, 40, 255];
    pub(super) const WHITE: [u8; 4] = [255, 255, 255, 255];
}

// The pixel at `x`, `y` of a picture of `size` in frame number `frame`.
#[cfg(target_os = "android")]
fn pixel_at(x: u32, y: u32, size: (u32, u32), frame: u32, split: Split) -> [u8; 4] {
    use colors::{BLUE, GREEN, RED, WHITE, YELLOW};

    // The corner squares' side, the bar's width, the bar's step in each frame and the grid's
    // spacing, in pixels
    const MARK: u32 = 48;
    const BAR: u32 = 24;
    const STEP: u32 = 6;
    const GRID: u32 = 64;

    let (width, height) = size;
    let left = x < MARK;
    let right = x + MARK >= width;
    let top = y < MARK;
    let bottom = y + MARK >= height;
    if top && left {
        return RED;
    }
    if top && right {
        return GREEN;
    }
    if bottom && left {
        return BLUE;
    }
    if bottom && right {
        return YELLOW;
    }
    if x.abs_diff((frame * STEP) % width) < BAR / 2 {
        return WHITE;
    }
    // A vertical gradient with a grid over it
    let shade = (y * 60 / height) as u8;
    let line = if x.is_multiple_of(GRID) || y.is_multiple_of(GRID) {
        30
    } else {
        0
    };
    let first_half = match split {
        Split::Whole => None,
        Split::LeftRight => Some(x < width / 2),
        Split::TopBottom => Some(y < height / 2),
    };
    let base = match first_half {
        None => [20, 30, 70],
        Some(true) => [120, 20, 20],
        Some(false) => [20, 20, 120],
    };
    let [r, g, b] = base.map(|channel: u8| channel.saturating_add(shade + line));
    [r, g, b, 255]
}
