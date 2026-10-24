#![expect(unsafe_code)]

use std::{
    borrow::Borrow,
    ffi::c_void,
    mem::zeroed,
    ptr,
    ptr::NonNull,
    slice,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use cranpose::framebuffer::{PixelRect, SurfaceError};
use esp_idf_svc::{
    hal::{
        delay::BLOCK,
        gpio::{self, AnyOutputPin, Level, Pin, PinDriver},
        spi::{
            SpiDriver,
            config::{Mode, Phase, Polarity},
        },
        task::queue::Queue,
        units::Hertz,
    },
    sys::{self, EspError, esp, esp_lcd_panel_handle_t, esp_lcd_panel_io_handle_t},
};

/// The colour encoding a panel reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PixelFormat {
    /// RGB565 with the low byte first, as QEMU's virtual panel reads it.
    Rgb565,
    /// RGB565 with the high byte first, as display controllers read it.
    Rgb565BigEndian,
}

/// One command of a display controller's start-up sequence.
#[derive(Clone, Copy, Debug)]
pub struct InitCommand {
    /// The command byte.
    pub command: u8,
    /// The parameter bytes.
    pub parameters: &'static [u8],
    /// The wait after the command, in milliseconds.
    pub delay_ms: u32,
}

impl InitCommand {
    /// A command that waits `delay_ms` after sending `parameters`.
    pub const fn new(command: u8, parameters: &'static [u8], delay_ms: u32) -> Self {
        Self {
            command,
            parameters,
            delay_ms,
        }
    }
}

/// A display controller that takes MIPI DCS commands, such as the ST7789,
/// ST7796, ILI9341 or ST77922: a column and a row command select a window of
/// its memory, and a memory write fills the window with RGB565 pixels.
#[derive(Clone, Copy, Debug)]
pub struct Controller {
    /// The width of the visible area in pixels.
    pub width: u32,
    /// The height of the visible area in pixels.
    pub height: u32,
    /// The column of the controller's memory where the visible area starts.
    pub x_offset: u32,
    /// The row of the controller's memory where the visible area starts.
    pub y_offset: u32,
    /// Update windows start and end on a multiple of this many columns: 4
    /// on the ST77922, 1 on controllers that take any window.
    pub column_alignment: u32,
    /// Update windows start and end on a multiple of this many rows.
    pub row_alignment: u32,
    /// The commands that set the controller up after its reset, in the glass
    /// vendor's values. They select 16-bit colour (`COLMOD`) and the scan
    /// direction (`MADCTL`), end the sleep (`SLPOUT`) and turn the display on
    /// (`DISPON`).
    pub init: &'static [InitCommand],
}

impl Controller {
    /// A `width` by `height` controller set up by `init`, whose visible area
    /// starts at the first column and row of its memory and takes any window.
    pub const fn new(width: u32, height: u32, init: &'static [InitCommand]) -> Self {
        Self {
            width,
            height,
            x_offset: 0,
            y_offset: 0,
            column_alignment: 1,
            row_alignment: 1,
            init,
        }
    }
}

/// The data lines a controller reads.
pub enum Lines<'d> {
    /// One data line, and a data/command select line that a low level sets
    /// to a command byte.
    Single {
        /// The data/command select.
        dc: AnyOutputPin<'d>,
    },
    /// Four data lines. Each command goes out on the first line behind the
    /// opcode 0x02, and pixels go out on all four behind the opcode 0x32.
    Quad,
}

/// A backlight that a GPIO switches.
pub struct Backlight<'d> {
    /// The switch.
    pub pin: AnyOutputPin<'d>,
    /// The level that turns the backlight on.
    pub on: Level,
}

/// How a display controller is wired to an SPI bus.
pub struct SpiWiring<'d> {
    /// The chip select.
    pub cs: AnyOutputPin<'d>,
    /// The data lines the controller reads.
    pub lines: Lines<'d>,
    /// The bus clock.
    pub clock: Hertz,
    /// The clock polarity and phase; most controllers read mode 0.
    pub mode: Mode,
    /// The reset line, which a low level resets. Without one, the software
    /// reset command resets the controller.
    pub reset: Option<AnyOutputPin<'d>>,
    /// The backlight, which turns on once the first picture is on the
    /// display, so the noise of the controller's memory never shows.
    pub backlight: Option<Backlight<'d>>,
}

const SWRESET: u8 = 0x01;
const CASET: u8 = 0x2A;
const RASET: u8 = 0x2B;
const RAMWR: u8 = 0x2C;
/// The opcode before a command of a quad SPI controller.
const QUAD_COMMAND: i32 = 0x02;
/// The opcode before a memory write whose pixels go out on four lines.
const QUAD_PIXELS: i32 = 0x32;
/// How long a reset line stays low, and how long the controller then takes
/// before it reads commands, as in the vendors' drivers.
const RESET_PULSE: Duration = Duration::from_millis(10);
const RESET_RECOVERY: Duration = Duration::from_millis(120);
/// Colour transfers that the panel IO queues before a draw call waits.
const TRANSFER_QUEUE_DEPTH: usize = 10;

/// A display that [`run`](crate::run) draws on.
pub struct Panel<'d> {
    device: Device<'d>,
    width: u32,
    height: u32,
    format: PixelFormat,
    column_alignment: u32,
    row_alignment: u32,
    backlight: Option<BacklightSwitch<'d>>,
}

enum Device<'d> {
    /// An `esp_lcd` panel driver that copies each update before its draw
    /// call returns.
    Driver(esp_lcd_panel_handle_t),
    /// A controller that reads each update by DMA after its draw call
    /// returns.
    Dcs(Dcs<'d>),
}

struct BacklightSwitch<'d> {
    pin: PinDriver<'d, gpio::Output>,
    on: Level,
    /// Whether the first picture is still to come.
    dark: bool,
}

/// A MIPI DCS controller behind an `esp_lcd` panel IO. A buffer stays in use
/// until the IO's callback counts its transfer.
struct Dcs<'d> {
    io: esp_lcd_panel_io_handle_t,
    quad: bool,
    x_offset: u32,
    y_offset: u32,
    transfers: Box<Transfers>,
    submitted: u32,
    /// The steps of the controller's setup still to run, until the first
    /// draw runs them.
    setup: Option<Setup>,
    /// Held high, after the reset, while the panel is open.
    reset: Option<PinDriver<'d, gpio::Output>>,
    /// The pins the panel IO drives.
    _cs: AnyOutputPin<'d>,
    _dc: Option<AnyOutputPin<'d>>,
    /// Kept open until the panel IO is closed.
    _bus: Box<dyn Borrow<SpiDriver<'d>> + 'd>,
}

/// The part of a controller's setup that waits: the release of its reset
/// line, then its commands, each once the wait before it has passed. It runs
/// before the first draw, so the waits pass while the application builds its
/// first picture.
struct Setup {
    /// Whether the reset line is still low.
    resetting: bool,
    /// The commands still to send.
    commands: &'static [InitCommand],
    /// When the next step may run.
    at: Instant,
}

/// The completion count that the panel IO's interrupt callback advances.
struct Transfers {
    done: AtomicU32,
    signal: Queue<u8>,
}

unsafe extern "C" fn color_transfer_done(
    _io: esp_lcd_panel_io_handle_t,
    _event: *mut sys::esp_lcd_panel_io_event_data_t,
    transfers: *mut c_void,
) -> bool {
    // SAFETY: `user_ctx` points into the `Transfers` box, which outlives the
    // panel IO that calls this.
    let transfers = unsafe { &*transfers.cast::<Transfers>() };
    transfers.done.fetch_add(1, Ordering::Release);
    // A full slot already holds a token for the waiter.
    transfers.signal.send_back(0, 0).unwrap_or(false)
}

fn spi_mode(mode: Mode) -> i32 {
    let polarity = i32::from(mode.polarity == Polarity::IdleHigh);
    let phase = i32::from(mode.phase == Phase::CaptureOnSecondTransition);
    polarity << 1 | phase
}

fn pin_number(pin: &impl Pin) -> i32 {
    i32::from(pin.pin())
}

fn invalid_size() -> EspError {
    EspError::from_infallible::<{ sys::ESP_ERR_INVALID_SIZE }>()
}

impl Dcs<'_> {
    /// Runs the steps of the setup still to run, each once its wait has
    /// passed.
    #[cold]
    fn finish_setup(&mut self) -> Result<(), EspError> {
        let Some(mut setup) = self.setup.take() else {
            return Ok(());
        };
        loop {
            std::thread::sleep(setup.at.saturating_duration_since(Instant::now()));
            let wait = if setup.resetting {
                if let Some(reset) = self.reset.as_mut() {
                    reset.set_high()?;
                }
                setup.resetting = false;
                RESET_RECOVERY
            } else {
                let Some((command, rest)) = setup.commands.split_first() else {
                    return Ok(());
                };
                self.send(command.command, command.parameters)?;
                setup.commands = rest;
                Duration::from_millis(command.delay_ms.into())
            };
            setup.at = Instant::now() + wait;
        }
    }

    fn send(&self, command: u8, parameters: &[u8]) -> Result<(), EspError> {
        let command = if self.quad {
            QUAD_COMMAND << 24 | i32::from(command) << 8
        } else {
            i32::from(command)
        };
        esp!(unsafe {
            sys::esp_lcd_panel_io_tx_param(
                self.io,
                command,
                parameters.as_ptr().cast(),
                parameters.len(),
            )
        })
    }

    /// Selects `area` of the visible pixels and queues `pixels` for it.
    fn draw(&mut self, area: PixelRect, pixels: &[u16]) -> Result<(), EspError> {
        if self.setup.is_some() {
            self.finish_setup()?;
        }
        let window = |start: u32, size: u32| {
            let [start_high, start_low] = (start as u16).to_be_bytes();
            let [end_high, end_low] = ((start + size - 1) as u16).to_be_bytes();
            [start_high, start_low, end_high, end_low]
        };
        self.send(CASET, &window(area.x + self.x_offset, area.width))?;
        self.send(RASET, &window(area.y + self.y_offset, area.height))?;
        let command = if self.quad {
            QUAD_PIXELS << 24 | i32::from(RAMWR) << 8
        } else {
            i32::from(RAMWR)
        };
        esp!(unsafe {
            sys::esp_lcd_panel_io_tx_color(
                self.io,
                command,
                pixels.as_ptr().cast(),
                size_of_val(pixels),
            )
        })?;
        self.submitted = self.submitted.wrapping_add(1);
        Ok(())
    }

    fn in_flight(&self) -> u32 {
        self.submitted
            .wrapping_sub(self.transfers.done.load(Ordering::Acquire))
    }

    /// Waits until at most `transfers` transfers are on their way.
    fn wait_for(&self, transfers: u32) {
        while self.in_flight() > transfers {
            let _ = self.transfers.signal.recv_front(BLOCK);
        }
    }
}

impl Drop for Dcs<'_> {
    fn drop(&mut self) {
        if let Err(error) = esp!(unsafe { sys::esp_lcd_panel_io_del(self.io) }) {
            log::warn!("closing the panel IO failed: {error}");
        }
    }
}

impl<'d> Panel<'d> {
    /// Opens QEMU's virtual RGB panel at `width` by `height` pixels, shown by
    /// `idf.py qemu --graphics` or `qemu-system-xtensa -display`. Real
    /// hardware has no such panel and returns `ESP_ERR_NOT_SUPPORTED`.
    pub fn qemu(width: u32, height: u32) -> Result<Self, EspError> {
        let config = sys::esp_lcd_rgb_qemu_config_t {
            width,
            height,
            bpp: sys::esp_lcd_rgb_qemu_bpp_t_RGB_QEMU_BPP_16,
        };
        let mut handle: esp_lcd_panel_handle_t = ptr::null_mut();
        esp!(unsafe { sys::esp_lcd_new_rgb_qemu(&config, &mut handle) })?;
        let panel = Self {
            device: Device::Driver(handle),
            width,
            height,
            format: PixelFormat::Rgb565,
            column_alignment: 1,
            row_alignment: 1,
            backlight: None,
        };
        esp!(unsafe { sys::esp_lcd_panel_init(handle) })?;
        Ok(panel)
    }

    /// Opens `controller` on `bus`, wired as `wiring` says: turns the
    /// backlight off and starts the controller's reset. The first draw sends
    /// the setup, each command after the wait the one before it asks for, so
    /// the waits pass while the application builds its first picture.
    ///
    /// Give the bus a DMA transfer size of at least one strip of the drawing
    /// pass, `strip_rows * width * 2` bytes (see
    /// [`DisplayConfig::strip_rows`](crate::DisplayConfig::strip_rows)): a
    /// smaller one splits each strip into several transfers. Other devices,
    /// such as a touch controller, may share the bus.
    pub fn spi(
        bus: impl Borrow<SpiDriver<'d>> + 'd,
        wiring: SpiWiring<'d>,
        controller: Controller,
    ) -> Result<Self, EspError> {
        let backlight = wiring
            .backlight
            .map(|backlight| {
                let mut pin = PinDriver::output(backlight.pin)?;
                pin.set_level(!backlight.on)?;
                Ok::<_, EspError>(BacklightSwitch {
                    pin,
                    on: backlight.on,
                    dark: true,
                })
            })
            .transpose()?;
        let signal = Queue::new(1);
        if signal.as_raw().is_null() {
            return Err(EspError::from_infallible::<{ sys::ESP_ERR_NO_MEM }>());
        }
        let transfers = Box::new(Transfers {
            done: AtomicU32::new(0),
            signal,
        });
        let (quad, dc) = match wiring.lines {
            Lines::Single { dc } => (false, Some(dc)),
            Lines::Quad => (true, None),
        };
        let mut config: sys::esp_lcd_panel_io_spi_config_t = unsafe { zeroed() };
        config.cs_gpio_num = pin_number(&wiring.cs);
        config.dc_gpio_num = dc.as_ref().map_or(-1, pin_number);
        config.spi_mode = spi_mode(wiring.mode);
        config.pclk_hz = wiring.clock.0;
        config.trans_queue_depth = TRANSFER_QUEUE_DEPTH;
        config.on_color_trans_done = Some(color_transfer_done);
        config.user_ctx = ptr::from_ref(&*transfers).cast_mut().cast();
        config.lcd_cmd_bits = if quad { 32 } else { 8 };
        config.lcd_param_bits = 8;
        config.flags.set_quad_mode(quad.into());
        let host = bus.borrow().host();
        let mut io: esp_lcd_panel_io_handle_t = ptr::null_mut();
        esp!(unsafe { sys::esp_lcd_new_panel_io_spi(host as _, &config, &mut io) })?;
        let mut dcs = Dcs {
            io,
            quad,
            x_offset: controller.x_offset,
            y_offset: controller.y_offset,
            transfers,
            submitted: 0,
            setup: None,
            reset: None,
            _cs: wiring.cs,
            _dc: dc,
            _bus: Box::new(bus),
        };
        let resetting = match wiring.reset {
            Some(pin) => {
                let mut reset = PinDriver::output(pin)?;
                reset.set_low()?;
                dcs.reset = Some(reset);
                true
            }
            None => {
                dcs.send(SWRESET, &[])?;
                false
            }
        };
        dcs.setup = Some(Setup {
            resetting,
            commands: controller.init,
            at: Instant::now()
                + if resetting {
                    RESET_PULSE
                } else {
                    RESET_RECOVERY
                },
        });
        Ok(Self {
            device: Device::Dcs(dcs),
            width: controller.width,
            height: controller.height,
            format: PixelFormat::Rgb565BigEndian,
            column_alignment: controller.column_alignment.max(1),
            row_alignment: controller.row_alignment.max(1),
            backlight,
        })
    }

    /// Returns the panel width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the panel height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the colour encoding the panel reads.
    pub(crate) fn format(&self) -> PixelFormat {
        self.format
    }

    /// Returns the rows of a strip of `rows` rows or fewer that keep a strip
    /// on the rows the panel accepts as an update.
    pub(crate) fn aligned_rows(&self, rows: u32) -> u32 {
        (rows - rows % self.row_alignment).max(self.row_alignment)
    }

    /// Returns `area` widened to the columns and rows the panel accepts as an
    /// update.
    pub(crate) fn align(&self, area: PixelRect) -> PixelRect {
        let widen = |start: u32, size: u32, alignment: u32, limit: u32| {
            let first = start - start % alignment;
            let end = (start + size).next_multiple_of(alignment).min(limit);
            (first, end - first)
        };
        let (x, width) = widen(area.x, area.width, self.column_alignment, self.width);
        let (y, height) = widen(area.y, area.height, self.row_alignment, self.height);
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns whether the controller's setup still waits to run, which the
    /// first draw finishes.
    pub(crate) fn is_setting_up(&self) -> bool {
        matches!(&self.device, Device::Dcs(dcs) if dcs.setup.is_some())
    }

    /// Returns how many pixel buffers the panel needs so that the next strip
    /// is drawn while the last one is on its way.
    pub(crate) fn buffers(&self) -> usize {
        match self.device {
            Device::Driver(_) => 1,
            Device::Dcs(_) => 2,
        }
    }

    /// Waits until at most one transfer is on its way, which leaves the
    /// buffer before the last one free to fill.
    pub(crate) fn wait_for_free_buffer(&self) {
        if let Device::Dcs(dcs) = &self.device {
            dcs.wait_for(1);
        }
    }

    pub(crate) fn draw_rect(
        &mut self,
        area: PixelRect,
        pixels: &TransferBuffer,
    ) -> Result<(), EspError> {
        let len = area.width as usize * area.height as usize;
        if len > pixels.len {
            return Err(invalid_size());
        }
        match &mut self.device {
            Device::Driver(handle) => esp!(unsafe {
                sys::esp_lcd_panel_draw_bitmap(
                    *handle,
                    area.x as i32,
                    area.y as i32,
                    (area.x + area.width) as i32,
                    (area.y + area.height) as i32,
                    pixels.pixels.as_ptr().cast(),
                )
            }),
            Device::Dcs(dcs) => dcs.draw(area, &pixels.as_slice()[..len]),
        }
    }

    /// Ends a picture. The first one turns the backlight on once its last
    /// strip has reached the panel.
    pub(crate) fn finish_picture(&mut self) -> Result<(), EspError> {
        let Some(backlight) = self.backlight.as_mut().filter(|backlight| backlight.dark) else {
            return Ok(());
        };
        if let Device::Dcs(dcs) = &self.device {
            dcs.wait_for(0);
        }
        backlight.dark = false;
        backlight.pin.set_level(backlight.on)
    }
}

impl Drop for Panel<'_> {
    fn drop(&mut self) {
        if let Device::Driver(handle) = self.device
            && let Err(error) = esp!(unsafe { sys::esp_lcd_panel_del(handle) })
        {
            log::warn!("closing the panel failed: {error}");
        }
    }
}

pub(crate) struct TransferBuffer {
    pixels: NonNull<u16>,
    len: usize,
}

impl TransferBuffer {
    pub(crate) fn new(len: usize) -> Result<Self, SurfaceError> {
        let caps = sys::MALLOC_CAP_DMA | sys::MALLOC_CAP_INTERNAL;
        let memory = unsafe { sys::heap_caps_calloc(len, size_of::<u16>(), caps) };
        let pixels = NonNull::new(memory.cast::<u16>()).ok_or(SurfaceError)?;
        Ok(Self { pixels, len })
    }

    fn as_slice(&self) -> &[u16] {
        unsafe { slice::from_raw_parts(self.pixels.as_ptr(), self.len) }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u16] {
        unsafe { slice::from_raw_parts_mut(self.pixels.as_ptr(), self.len) }
    }
}

impl Drop for TransferBuffer {
    fn drop(&mut self) {
        unsafe { sys::heap_caps_free(self.pixels.as_ptr().cast()) };
    }
}
