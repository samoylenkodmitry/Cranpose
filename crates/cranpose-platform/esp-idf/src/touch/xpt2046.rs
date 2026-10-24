use std::borrow::Borrow;

use esp_idf_svc::{
    hal::{
        gpio::AnyInputPin,
        spi::{SpiDeviceDriver, SpiDriver},
    },
    sys::EspError,
};

use super::{Interrupt, Orientation, Touch};

/// How raw readings of an XPT2046 map to display pixels: an axis starts at
/// its `offset`, and `span` raw steps cover the display along it.
#[derive(Clone, Copy, Debug)]
pub struct Xpt2046Calibration {
    /// The raw X reading at the start of the controller's X axis.
    pub x_offset: u16,
    /// The raw X steps along the display.
    pub x_span: u16,
    /// The raw Y reading at the start of the controller's Y axis.
    pub y_offset: u16,
    /// The raw Y steps along the display.
    pub y_span: u16,
    /// How the controller's axes lie on the display.
    pub orientation: Orientation,
    /// The pressure reading, `4095 + z1 - z2`, above which a finger presses.
    pub pressure_threshold: u16,
}

/// The control bytes that start a 12-bit conversion of a channel.
const READ_X: u8 = 0xD0;
const READ_Y: u8 = 0x90;
const READ_PRESSURE_1: u8 = 0xB0;
const READ_PRESSURE_2: u8 = 0xC0;
const FULL_SCALE: i32 = 4095;

/// An XPT2046 resistive touch controller on an SPI bus, which it often
/// shares with the display.
pub struct Xpt2046Touch<'d, T>
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    device: SpiDeviceDriver<'d, T>,
    interrupt: Interrupt<'d>,
    /// The display pixels along the controller's X and Y axes.
    size: (u16, u16),
    calibration: Xpt2046Calibration,
}

impl<'d, T> Xpt2046Touch<'d, T>
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    /// Reads the controller through `device`, at a clock of at most 2.5 MHz,
    /// with its pen interrupt on `interrupt`. `width` and `height` are the
    /// display's size in pixels.
    pub fn new(
        device: SpiDeviceDriver<'d, T>,
        interrupt: impl Into<AnyInputPin<'d>>,
        width: u16,
        height: u16,
        calibration: Xpt2046Calibration,
    ) -> Result<Self, EspError> {
        let size = if calibration.orientation.swap_xy {
            (height, width)
        } else {
            (width, height)
        };
        Ok(Self {
            device,
            interrupt: Interrupt::new(interrupt.into()),
            size,
            calibration,
        })
    }

    /// Converts `control`'s channel once and returns the 12-bit result.
    fn convert(&mut self, control: u8) -> Result<i32, EspError> {
        let mut reading = [0u8; 3];
        self.device.transfer(&mut reading, &[control, 0, 0])?;
        let [_, high, low] = reading;
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
fn scale(raw: i32, offset: u16, span: u16, size: u16) -> u16 {
    let span = i32::from(span).max(1);
    let steps = (raw - i32::from(offset)).clamp(0, span - 1);
    let position = i64::from(steps) * i64::from(size) / i64::from(span);
    u16::try_from(position).unwrap_or(u16::MAX)
}

impl<'d, T> Touch for Xpt2046Touch<'d, T>
where
    T: Borrow<SpiDriver<'d>> + 'd,
{
    fn arm<F>(&mut self, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        // The controller pulls the line up itself, and the input-only pins
        // it often sits on have no pull-up resistor.
        self.interrupt.arm(false, wake)
    }

    fn read(&mut self) -> Result<Option<(u16, u16)>, EspError> {
        let pressure =
            FULL_SCALE + self.convert(READ_PRESSURE_1)? - self.convert(READ_PRESSURE_2)?;
        let calibration = self.calibration;
        if pressure <= i32::from(calibration.pressure_threshold) {
            return Ok(None);
        }
        let x = self.measure(READ_X)?;
        let y = self.measure(READ_Y)?;
        let point = (
            scale(x, calibration.x_offset, calibration.x_span, self.size.0),
            scale(y, calibration.y_offset, calibration.y_span, self.size.1),
        );
        Ok(Some(calibration.orientation.place(point, self.size)))
    }
}
