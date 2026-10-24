#![expect(unsafe_code)]

use std::{mem::zeroed, ptr};

use esp_idf_svc::sys::{self, EspError, esp};

use super::{Interrupt, Touch};
use crate::panel::SPI_HOST;

/// The pins of an XPT2046 touch controller on the display's SPI bus.
#[derive(Clone, Copy, Debug)]
pub struct Xpt2046Pins {
    /// The chip select.
    pub cs: i32,
    /// The pen interrupt, which goes low while a finger presses.
    pub interrupt: i32,
}

/// How raw readings of the XPT2046 map to display pixels: an axis starts at
/// `offset`, and `span` raw steps cover the display along it.
#[derive(Clone, Copy, Debug)]
pub struct Xpt2046Calibration {
    /// The raw X reading at the display's left edge.
    pub x_offset: u16,
    /// The raw X steps across the display's width.
    pub x_span: u16,
    /// The raw Y reading at the display's top edge.
    pub y_offset: u16,
    /// The raw Y steps across the display's height.
    pub y_span: u16,
    /// Whether raw X grows from the display's right edge to its left.
    pub invert_x: bool,
    /// Whether raw Y grows from the display's bottom edge to its top.
    pub invert_y: bool,
}

const CLOCK_HZ: i32 = 2_000_000;
/// The control bytes that start a 12-bit conversion of a channel.
const READ_X: u8 = 0xD0;
const READ_Y: u8 = 0x90;
const READ_PRESSURE_1: u8 = 0xB0;
const READ_PRESSURE_2: u8 = 0xC0;
/// The pressure reading, `4095 + z1 - z2`, above which a finger presses.
const PRESSURE_THRESHOLD: i32 = 600;
const FULL_SCALE: i32 = 4095;

/// An XPT2046 resistive touch controller, which shares the SPI bus of the
/// display that opened it.
pub struct Xpt2046Touch {
    device: sys::spi_device_handle_t,
    interrupt: Interrupt,
    width: u32,
    height: u32,
    calibration: Xpt2046Calibration,
}

impl Xpt2046Touch {
    /// Opens the controller on the bus of the display, which must be open
    /// already. `width` and `height` are the display's size in pixels.
    pub fn new(
        pins: Xpt2046Pins,
        width: u32,
        height: u32,
        calibration: Xpt2046Calibration,
    ) -> Result<Self, EspError> {
        let mut config: sys::spi_device_interface_config_t = unsafe { zeroed() };
        config.clock_speed_hz = CLOCK_HZ;
        config.mode = 0;
        config.spics_io_num = pins.cs;
        config.queue_size = 1;
        let mut device: sys::spi_device_handle_t = ptr::null_mut();
        esp!(unsafe { sys::spi_bus_add_device(SPI_HOST, &config, &mut device) })?;
        Ok(Self {
            device,
            interrupt: Interrupt::new(pins.interrupt),
            width,
            height,
            calibration,
        })
    }

    /// Converts `control`'s channel once and returns the 12-bit result.
    fn convert(&mut self, control: u8) -> Result<i32, EspError> {
        let mut transaction: sys::spi_transaction_t = unsafe { zeroed() };
        transaction.flags = sys::SPI_TRANS_USE_TXDATA | sys::SPI_TRANS_USE_RXDATA;
        transaction.length = 24;
        transaction.__bindgen_anon_1.tx_data = [control, 0, 0, 0];
        esp!(unsafe { sys::spi_device_polling_transmit(self.device, &mut transaction) })?;
        let [_, high, low, _] = unsafe { transaction.__bindgen_anon_2.rx_data };
        Ok(i32::from(u16::from_be_bytes([high, low]) >> 3 & 0x0FFF))
    }

    /// Returns the median of three conversions, which drops a noisy one.
    fn measure(&mut self, control: u8) -> Result<i32, EspError> {
        let a = self.convert(control)?;
        let b = self.convert(control)?;
        let c = self.convert(control)?;
        Ok(a.min(b).max(a.max(b).min(c)))
    }
}

/// Maps `raw` onto `0..size`, where `span` raw steps from `offset` cover the
/// axis.
fn scale(raw: i32, offset: u16, span: u16, size: u32, invert: bool) -> u16 {
    let span = i32::from(span).max(1);
    let steps = (raw - i32::from(offset)).clamp(0, span - 1);
    let last = i64::from(size.saturating_sub(1));
    let position = i64::from(steps) * i64::from(size) / i64::from(span);
    let position = if invert {
        last - position.min(last)
    } else {
        position.min(last)
    };
    u16::try_from(position).unwrap_or(u16::MAX)
}

impl Touch for Xpt2046Touch {
    fn arm<F>(&mut self, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        // The controller pulls the line up itself, and the pin has no
        // pull-up resistor of its own.
        self.interrupt.arm(false, wake)
    }

    fn read(&mut self) -> Result<Option<(u16, u16)>, EspError> {
        let pressure =
            FULL_SCALE + self.convert(READ_PRESSURE_1)? - self.convert(READ_PRESSURE_2)?;
        if pressure <= PRESSURE_THRESHOLD {
            return Ok(None);
        }
        let x = self.measure(READ_X)?;
        let y = self.measure(READ_Y)?;
        let calibration = self.calibration;
        Ok(Some((
            scale(
                x,
                calibration.x_offset,
                calibration.x_span,
                self.width,
                calibration.invert_x,
            ),
            scale(
                y,
                calibration.y_offset,
                calibration.y_span,
                self.height,
                calibration.invert_y,
            ),
        )))
    }
}

impl Drop for Xpt2046Touch {
    fn drop(&mut self) {
        if let Err(error) = esp!(unsafe { sys::spi_bus_remove_device(self.device) }) {
            log::warn!("closing the touch controller failed: {error}");
        }
    }
}
