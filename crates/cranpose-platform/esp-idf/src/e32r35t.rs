//! The 3.5-inch ESP32-32E display board, LCDWiki's E32R35T: an ESP32-D0WD-V3
//! with 4 MB of flash and no PSRAM behind a 320 by 480 display with an
//! ST7796 controller on an SPI bus. A resistive XPT2046 touch controller
//! shares the bus.

use std::time::Duration;

use esp_idf_svc::sys::EspError;

use crate::{
    Board, DisplayConfig, InitCommand, Panel, SpiPins, Xpt2046Calibration, Xpt2046Pins,
    Xpt2046Touch, panel::command,
};

/// The display width in pixels.
pub const WIDTH: u32 = 320;
/// The display height in pixels.
pub const HEIGHT: u32 = 480;

/// The rows of a strip. The chip has 520 KB of SRAM and no PSRAM, and a strip
/// takes 10 bytes of it for each pixel; the composition of the demo takes
/// 150 KB.
const STRIP_ROWS: u32 = 8;

const DISPLAY: SpiPins = SpiPins {
    cs: 15,
    clk: 14,
    mosi: 13,
    miso: 12,
    dc: 2,
    backlight: 27,
};

const TOUCH: Xpt2046Pins = Xpt2046Pins {
    cs: 33,
    interrupt: 36,
};

/// The board vendor's calibration of the touch panel against the display.
const CALIBRATION: Xpt2046Calibration = Xpt2046Calibration {
    x_offset: 263,
    x_span: 3443,
    y_offset: 256,
    y_span: 3643,
    invert_x: false,
    invert_y: true,
};

/// The controller's setup: the scan direction and colour order of the
/// portrait view, then the voltages and gamma of the glass.
const INIT: &[InitCommand] = &[
    command(0x36, &[0x48], 0),
    command(0xF0, &[0xC3], 0),
    command(0xF0, &[0x96], 0),
    command(0xB4, &[0x01], 0),
    command(0xB7, &[0xC6], 0),
    command(0xE8, &[0x40, 0x8A, 0x00, 0x00, 0x29, 0x19, 0xA5, 0x33], 0),
    command(0xC1, &[0x06], 0),
    command(0xC2, &[0xA7], 0),
    command(0xC5, &[0x18], 0),
    command(
        0xE0,
        &[
            0xF0, 0x09, 0x0B, 0x06, 0x04, 0x15, 0x2F, 0x54, 0x42, 0x3C, 0x17, 0x14, 0x18, 0x1B,
        ],
        0,
    ),
    command(
        0xE1,
        &[
            0xF0, 0x09, 0x0B, 0x06, 0x04, 0x03, 0x2D, 0x43, 0x42, 0x3B, 0x16, 0x14, 0x17, 0x1B,
        ],
        0,
    ),
    command(0xF0, &[0x3C], 0),
    command(0xF0, &[0x69], 0),
];

/// The board.
#[derive(Clone, Copy, Debug)]
pub struct E32r35t;

impl Board for E32r35t {
    type Touch = Xpt2046Touch;

    fn open() -> Result<(Panel, Xpt2046Touch), EspError> {
        let panel = Panel::st7796_spi(WIDTH, HEIGHT, STRIP_ROWS, DISPLAY, Some(INIT))?;
        let touch = Xpt2046Touch::new(TOUCH, WIDTH, HEIGHT, CALIBRATION)?;
        Ok((panel, touch))
    }

    /// The chip takes 16.7 ms to compose the demo's animated frame, which
    /// leaves the task no time to sleep at 60 frames a second: the idle task
    /// starves and the watchdog resets the chip.
    fn display_config() -> DisplayConfig {
        DisplayConfig {
            frame_interval: Duration::from_millis(33),
            ..DisplayConfig::default()
        }
    }
}
