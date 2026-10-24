//! The 3.5-inch ESP32-32E display board, LCDWiki's E32R35T: an ESP32-D0WD-V3
//! with 4 MB of flash and no PSRAM behind a 320 by 480 display with an
//! ST7796 controller on an SPI bus. A resistive XPT2046 touch controller
//! shares the bus.

use std::rc::Rc;

use cranpose_platform_esp_idf::{
    Backlight, Controller, DisplayConfig, InitCommand, Lines, Orientation, Panel, SpiWiring,
    Xpt2046Calibration, Xpt2046Touch,
};
use esp_idf_svc::{
    hal::{
        gpio::Level,
        peripherals::Peripherals,
        spi::{Dma, SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig, config::MODE_0},
        units::Hertz,
    },
    sys::EspError,
};

/// The demo's title.
pub const TITLE: &str = "Cranpose on ESP32";

const WIDTH: u16 = 320;
const HEIGHT: u16 = 480;

/// The rows of a strip. The chip has 520 KB of SRAM and no PSRAM, and a strip
/// takes 10 bytes of it for each pixel; the composition of the demo takes
/// 150 KB.
const STRIP_ROWS: u32 = 8;
/// The display's DMA transfers carry one strip.
const STRIP_BYTES: usize = WIDTH as usize * STRIP_ROWS as usize * 2;

/// The board vendor's calibration of the touch panel against the display:
/// raw Y grows from the bottom edge.
const CALIBRATION: Xpt2046Calibration = Xpt2046Calibration {
    x_offset: 263,
    x_span: 3443,
    y_offset: 256,
    y_span: 3643,
    orientation: Orientation {
        swap_xy: false,
        mirror_x: false,
        mirror_y: true,
    },
    pressure_threshold: 600,
};

/// The controller's setup: sleep out, then the scan direction and colour
/// order of the portrait view, the voltages and gamma of the glass, and the
/// display on.
const INIT: &[InitCommand] = &[
    InitCommand::new(0x11, &[], 100),
    InitCommand::new(0x36, &[0x00], 0),
    InitCommand::new(0x3A, &[0x05], 0),
    InitCommand::new(0x36, &[0x48], 0),
    InitCommand::new(0xF0, &[0xC3], 0),
    InitCommand::new(0xF0, &[0x96], 0),
    InitCommand::new(0xB4, &[0x01], 0),
    InitCommand::new(0xB7, &[0xC6], 0),
    InitCommand::new(0xE8, &[0x40, 0x8A, 0x00, 0x00, 0x29, 0x19, 0xA5, 0x33], 0),
    InitCommand::new(0xC1, &[0x06], 0),
    InitCommand::new(0xC2, &[0xA7], 0),
    InitCommand::new(0xC5, &[0x18], 0),
    InitCommand::new(
        0xE0,
        &[
            0xF0, 0x09, 0x0B, 0x06, 0x04, 0x15, 0x2F, 0x54, 0x42, 0x3C, 0x17, 0x14, 0x18, 0x1B,
        ],
        0,
    ),
    InitCommand::new(
        0xE1,
        &[
            0xF0, 0x09, 0x0B, 0x06, 0x04, 0x03, 0x2D, 0x43, 0x42, 0x3B, 0x16, 0x14, 0x17, 0x1B,
        ],
        0,
    ),
    InitCommand::new(0xF0, &[0x3C], 0),
    InitCommand::new(0xF0, &[0x69], 0),
    InitCommand::new(0x29, &[], 0),
];

const CONTROLLER: Controller = Controller::new(WIDTH as u32, HEIGHT as u32, INIT);

/// The display settings of the chip.
pub fn config() -> DisplayConfig {
    DisplayConfig {
        strip_rows: STRIP_ROWS,
        ..DisplayConfig::default()
    }
}

/// Opens the display and the touch controller on the shared SPI bus.
pub fn open() -> Result<
    (
        Panel<'static>,
        Xpt2046Touch<'static, Rc<SpiDriver<'static>>>,
    ),
    EspError,
> {
    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;
    let bus = Rc::new(SpiDriver::new(
        peripherals.spi2,
        pins.gpio14,
        pins.gpio13,
        Some(pins.gpio12),
        &SpiDriverConfig::new().dma(Dma::Auto(STRIP_BYTES)),
    )?);
    let wiring = SpiWiring {
        cs: pins.gpio15.into(),
        lines: Lines::Single {
            dc: pins.gpio2.into(),
        },
        clock: Hertz(40_000_000),
        mode: MODE_0,
        reset: None,
        backlight: Some(Backlight {
            pin: pins.gpio27.into(),
            on: Level::High,
        }),
    };
    let panel = Panel::spi(Rc::clone(&bus), wiring, CONTROLLER)?;
    let touch_device = SpiDeviceDriver::new(
        bus,
        Some(pins.gpio33),
        &SpiConfig::new().baudrate(Hertz(2_000_000)),
    )?;
    let touch = Xpt2046Touch::new(touch_device, pins.gpio36, WIDTH, HEIGHT, CALIBRATION)?;
    Ok((panel, touch))
}
