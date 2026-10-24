#![expect(unsafe_code)]

use std::{
    ffi::c_void,
    mem::zeroed,
    ptr,
    ptr::NonNull,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};

use cranpose::framebuffer::{PixelRect, SurfaceError};
use esp_idf_svc::{
    hal::{delay::BLOCK, task::queue::Queue},
    sys::{self, EspError, esp, esp_lcd_panel_handle_t, esp_lcd_panel_io_handle_t},
};

/// The colour encoding a panel reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGB565 with the low byte first, as QEMU's virtual panel reads it.
    Rgb565,
    /// RGB565 with the high byte first, as SPI panels read it.
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

/// An init command that waits `delay_ms` after sending `parameters`.
pub(crate) const fn command(command: u8, parameters: &'static [u8], delay_ms: u32) -> InitCommand {
    InitCommand {
        command,
        parameters,
        delay_ms,
    }
}

/// The pins of a display on a quad SPI bus.
#[derive(Clone, Copy, Debug)]
pub struct QspiPins {
    /// The chip select.
    pub cs: i32,
    /// The bus clock.
    pub clk: i32,
    /// The four data lines, from `D0` to `D3`.
    pub data: [i32; 4],
    /// The backlight switch, which a high level turns on.
    pub backlight: i32,
}

/// The pins of a display on a single SPI bus.
#[derive(Clone, Copy, Debug)]
pub struct SpiPins {
    /// The chip select.
    pub cs: i32,
    /// The bus clock.
    pub clk: i32,
    /// The data line from the chip to the display.
    pub mosi: i32,
    /// The data line from the display to the chip.
    pub miso: i32,
    /// The data/command select, which a low level sets to a command byte.
    pub dc: i32,
    /// The backlight switch, which a high level turns on.
    pub backlight: i32,
}

/// The rows in a strip of a display with memory to spare.
pub const DEFAULT_STRIP_ROWS: u32 = 24;

/// The pixels of a strip of `rows` rows on a display `width` pixels wide.
fn strip_pixels(width: u32, rows: u32) -> usize {
    width as usize * rows.max(1) as usize
}

/// The bus that a display opens and a touch controller may share.
pub(crate) const SPI_HOST: sys::spi_host_device_t = sys::spi_host_device_t_SPI2_HOST;
const CLOCK_HZ: u32 = 40_000_000;
/// The ST77922 shows an update window only when it starts and ends on a
/// multiple of four columns.
const ST77922_COLUMN_ALIGNMENT: u32 = 4;
/// Colour transfers that the panel IO queues before a draw call waits.
const TRANSFER_QUEUE_DEPTH: usize = 10;

/// A display panel driven through ESP-IDF's `esp_lcd`.
pub struct Panel {
    handle: esp_lcd_panel_handle_t,
    link: Option<Link>,
    width: u32,
    height: u32,
    format: PixelFormat,
    /// Updates start and end on a multiple of this many columns.
    column_alignment: u32,
    /// The pixels of one strip: the drawing pass sends the panel this many
    /// at a time.
    strip_pixels: usize,
    backlight: Option<i32>,
}

/// An SPI connection. The panel reads a buffer by DMA after the draw call
/// returns, so the buffer stays in use until the callback counts the
/// transfer.
struct Link {
    io: Option<esp_lcd_panel_io_handle_t>,
    transfers: Box<Transfers>,
    submitted: u32,
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
    let transfers = unsafe { &*transfers.cast::<Transfers>() };
    transfers.done.fetch_add(1, Ordering::Release);
    // A full slot already holds a token for the waiter.
    transfers.signal.send_back(0, 0).unwrap_or(false)
}

impl Link {
    /// Opens the SPI bus, whose largest transfer is one strip of
    /// `strip_pixels` pixels.
    fn open(mut bus: sys::spi_bus_config_t, strip_pixels: usize) -> Result<Self, EspError> {
        let signal = Queue::new(1);
        if signal.as_raw().is_null() {
            return Err(EspError::from_infallible::<{ sys::ESP_ERR_NO_MEM }>());
        }
        bus.max_transfer_sz =
            i32::try_from(strip_pixels * size_of::<u16>()).map_err(|_| invalid_size())?;
        bus.data4_io_num = -1;
        bus.data5_io_num = -1;
        bus.data6_io_num = -1;
        bus.data7_io_num = -1;
        esp!(unsafe {
            sys::spi_bus_initialize(SPI_HOST, &bus, sys::spi_common_dma_t_SPI_DMA_CH_AUTO)
        })?;
        Ok(Self {
            io: None,
            transfers: Box::new(Transfers {
                done: AtomicU32::new(0),
                signal,
            }),
            submitted: 0,
        })
    }

    /// Opens the panel IO of the display on chip select `cs`. `dc` is the
    /// data/command pin, or -1 for the quad bus, whose commands are 32 bits
    /// wide.
    fn open_io(
        &mut self,
        cs: i32,
        dc: i32,
        quad: bool,
    ) -> Result<esp_lcd_panel_io_handle_t, EspError> {
        let mut config: sys::esp_lcd_panel_io_spi_config_t = unsafe { zeroed() };
        config.cs_gpio_num = cs;
        config.dc_gpio_num = dc;
        config.spi_mode = 0;
        config.pclk_hz = CLOCK_HZ;
        config.trans_queue_depth = TRANSFER_QUEUE_DEPTH;
        config.on_color_trans_done = Some(color_transfer_done);
        config.user_ctx = ptr::from_ref(&*self.transfers).cast_mut().cast();
        config.lcd_cmd_bits = if quad { 32 } else { 8 };
        config.lcd_param_bits = 8;
        config.flags.set_quad_mode(quad.into());
        let mut io: esp_lcd_panel_io_handle_t = ptr::null_mut();
        esp!(unsafe { sys::esp_lcd_new_panel_io_spi(SPI_HOST as _, &config, &mut io) })?;
        self.io = Some(io);
        Ok(io)
    }
}

fn invalid_size() -> EspError {
    EspError::from_infallible::<{ sys::ESP_ERR_INVALID_SIZE }>()
}

/// Copies `commands` into the C structs `sys::$command` that a panel driver
/// reads in `esp_lcd_panel_init`.
macro_rules! driver_commands {
    ($command:ident, $commands:expr) => {
        $commands
            .iter()
            .map(|command| sys::$command {
                cmd: command.command.into(),
                data: command.parameters.as_ptr().cast(),
                data_bytes: command.parameters.len(),
                delay_ms: command.delay_ms,
            })
            .collect::<Vec<_>>()
    };
}

/// The device configuration of a 16-bit panel with `vendor` config, whose
/// reset the chip's reset line does.
fn device_config(vendor: *mut c_void) -> sys::esp_lcd_panel_dev_config_t {
    let mut device: sys::esp_lcd_panel_dev_config_t = unsafe { zeroed() };
    device.reset_gpio_num = -1;
    device.bits_per_pixel = 16;
    device.vendor_config = vendor;
    device
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(io) = self.io
            && let Err(error) = esp!(unsafe { sys::esp_lcd_panel_io_del(io) })
        {
            log::warn!("closing the panel IO failed: {error}");
        }
        if let Err(error) = esp!(unsafe { sys::spi_bus_free(SPI_HOST) }) {
            log::warn!("freeing the SPI bus failed: {error}");
        }
    }
}

impl Panel {
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
            handle,
            link: None,
            width,
            height,
            format: PixelFormat::Rgb565,
            column_alignment: 1,
            strip_pixels: strip_pixels(width, DEFAULT_STRIP_ROWS),
            backlight: None,
        };
        esp!(unsafe { sys::esp_lcd_panel_init(panel.handle) })?;
        Ok(panel)
    }

    /// Opens a `width` by `height` display with an ST77922 controller on the
    /// quad SPI bus `pins` names. The drawing pass sends `strip_rows` rows at
    /// a time, which takes two DMA buffers and one RGBA buffer of that size
    /// from internal memory. `init` is the controller's setup for the glass;
    /// `None` takes the driver's default. The panel's reset line is
    /// the chip's reset. The backlight turns on after the first picture,
    /// which leaves no noise from the controller's memory on show.
    pub fn st77922_qspi(
        width: u32,
        height: u32,
        strip_rows: u32,
        pins: QspiPins,
        init: Option<&[InitCommand]>,
    ) -> Result<Self, EspError> {
        let mut bus: sys::spi_bus_config_t = unsafe { zeroed() };
        bus.__bindgen_anon_1.data0_io_num = pins.data[0];
        bus.__bindgen_anon_2.data1_io_num = pins.data[1];
        bus.__bindgen_anon_3.data2_io_num = pins.data[2];
        bus.__bindgen_anon_4.data3_io_num = pins.data[3];
        bus.sclk_io_num = pins.clk;
        let strip_pixels = strip_pixels(width, strip_rows);
        let mut link = Link::open(bus, strip_pixels)?;
        let io = link.open_io(pins.cs, -1, true)?;

        // The driver reads the commands only in `esp_lcd_panel_init`.
        let commands = driver_commands!(st77922_lcd_init_cmd_t, init.unwrap_or_default());
        let mut vendor: sys::st77922_vendor_config_t = unsafe { zeroed() };
        if init.is_some() {
            vendor.init_cmds = commands.as_ptr();
            vendor.init_cmds_size = u16::try_from(commands.len()).map_err(|_| invalid_size())?;
        }
        vendor.flags.set_use_qspi_interface(1);
        let device = device_config(ptr::from_mut(&mut vendor).cast());
        let mut handle: esp_lcd_panel_handle_t = ptr::null_mut();
        esp!(unsafe { sys::esp_lcd_new_panel_st77922(io, &device, &mut handle) })?;
        Self::start(
            handle,
            link,
            (width, height, strip_pixels),
            ST77922_COLUMN_ALIGNMENT,
            pins.backlight,
        )
    }

    /// Opens a `width` by `height` display with an ST7796 controller on the
    /// SPI bus `pins` names. The drawing pass sends `strip_rows` rows at a
    /// time, which takes two DMA buffers and one RGBA buffer of that size
    /// from internal memory. `init` is the controller's setup for the glass,
    /// which includes its scan direction and colour order; `None` takes the
    /// driver's default. The panel's reset line is the chip's reset. The
    /// backlight turns on after the first picture.
    pub fn st7796_spi(
        width: u32,
        height: u32,
        strip_rows: u32,
        pins: SpiPins,
        init: Option<&[InitCommand]>,
    ) -> Result<Self, EspError> {
        let mut bus: sys::spi_bus_config_t = unsafe { zeroed() };
        bus.__bindgen_anon_1.mosi_io_num = pins.mosi;
        bus.__bindgen_anon_2.miso_io_num = pins.miso;
        bus.__bindgen_anon_3.quadwp_io_num = -1;
        bus.__bindgen_anon_4.quadhd_io_num = -1;
        bus.sclk_io_num = pins.clk;
        let strip_pixels = strip_pixels(width, strip_rows);
        let mut link = Link::open(bus, strip_pixels)?;
        let io = link.open_io(pins.cs, pins.dc, false)?;

        let commands = driver_commands!(st7796_lcd_init_cmd_t, init.unwrap_or_default());
        let mut vendor: sys::st7796_vendor_config_t = unsafe { zeroed() };
        if init.is_some() {
            vendor.init_cmds = commands.as_ptr();
            vendor.init_cmds_size = u16::try_from(commands.len()).map_err(|_| invalid_size())?;
        }
        let device = device_config(ptr::from_mut(&mut vendor).cast());
        let mut handle: esp_lcd_panel_handle_t = ptr::null_mut();
        esp!(unsafe { sys::esp_lcd_new_panel_st7796(io, &device, &mut handle) })?;
        Self::start(
            handle,
            link,
            (width, height, strip_pixels),
            1,
            pins.backlight,
        )
    }

    /// Resets and starts the controller behind `handle`, which leaves the
    /// display on with the backlight off.
    fn start(
        handle: esp_lcd_panel_handle_t,
        link: Link,
        (width, height, strip_pixels): (u32, u32, usize),
        column_alignment: u32,
        backlight: i32,
    ) -> Result<Self, EspError> {
        let panel = Self {
            handle,
            link: Some(link),
            width,
            height,
            format: PixelFormat::Rgb565BigEndian,
            column_alignment,
            strip_pixels,
            backlight: Some(backlight),
        };
        esp!(unsafe { sys::esp_lcd_panel_reset(panel.handle) })?;
        esp!(unsafe { sys::esp_lcd_panel_init(panel.handle) })?;
        esp!(unsafe { sys::esp_lcd_panel_disp_on_off(panel.handle, true) })?;
        esp!(unsafe { sys::gpio_reset_pin(backlight) })?;
        esp!(unsafe { sys::gpio_set_direction(backlight, sys::gpio_mode_t_GPIO_MODE_OUTPUT) })?;
        esp!(unsafe { sys::gpio_set_level(backlight, 0) })?;
        Ok(panel)
    }

    /// Returns the panel width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the panel height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the pixels of one strip of the drawing pass.
    pub(crate) fn strip_pixels(&self) -> usize {
        self.strip_pixels
    }

    /// Returns the colour encoding the panel reads.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// Returns `area` widened to the columns the panel accepts as an update.
    pub(crate) fn align(&self, area: PixelRect) -> PixelRect {
        let alignment = self.column_alignment;
        let left = area.x - area.x % alignment;
        let right = (area.x + area.width)
            .next_multiple_of(alignment)
            .min(self.width);
        PixelRect {
            x: left,
            width: right - left,
            ..area
        }
    }

    /// Returns how many pixel buffers the panel needs so that the next strip
    /// is drawn while the last one is on its way.
    pub(crate) fn buffers(&self) -> usize {
        if self.link.is_some() { 2 } else { 1 }
    }

    /// Waits until at most one transfer is on its way, which leaves the
    /// buffer before the last one free to fill.
    pub(crate) fn wait_for_free_buffer(&self) {
        if let Some(link) = &self.link {
            while link
                .submitted
                .wrapping_sub(link.transfers.done.load(Ordering::Acquire))
                > 1
            {
                let _ = link.transfers.signal.recv_front(BLOCK);
            }
        }
    }

    pub(crate) fn draw_rect(
        &mut self,
        area: PixelRect,
        pixels: &TransferBuffer,
    ) -> Result<(), EspError> {
        if area.width as usize * area.height as usize > pixels.len {
            return Err(EspError::from_infallible::<{ sys::ESP_ERR_INVALID_SIZE }>());
        }
        esp!(unsafe {
            sys::esp_lcd_panel_draw_bitmap(
                self.handle,
                area.x as i32,
                area.y as i32,
                (area.x + area.width) as i32,
                (area.y + area.height) as i32,
                pixels.pixels.as_ptr().cast(),
            )
        })?;
        if let Some(link) = &mut self.link {
            link.submitted = link.submitted.wrapping_add(1);
        }
        Ok(())
    }

    /// Ends a picture. The first one turns the backlight on once its last
    /// strip has reached the panel.
    pub(crate) fn finish_picture(&mut self) -> Result<(), EspError> {
        let Some(pin) = self.backlight.take() else {
            return Ok(());
        };
        if let Some(link) = &self.link {
            while link.transfers.done.load(Ordering::Acquire) != link.submitted {
                let _ = link.transfers.signal.recv_front(BLOCK);
            }
        }
        esp!(unsafe { sys::gpio_set_level(pin, 1) })
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

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u16] {
        unsafe { slice::from_raw_parts_mut(self.pixels.as_ptr(), self.len) }
    }
}

impl Drop for TransferBuffer {
    fn drop(&mut self) {
        unsafe { sys::heap_caps_free(self.pixels.as_ptr().cast()) };
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Err(error) = esp!(unsafe { sys::esp_lcd_panel_del(self.handle) }) {
            log::warn!("closing the panel failed: {error}");
        }
    }
}
