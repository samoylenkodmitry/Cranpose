#![expect(unsafe_code)]

use std::time::{Duration, Instant};

use esp_idf_svc::{
    hal::{
        delay::{FreeRtos, TickType},
        gpio::AnyIOPin,
        i2c::{I2C0, I2cConfig, I2cDriver},
        units::Hertz,
    },
    sys::{self, EspError, esp},
};

use super::{Interrupt, Touch};

/// The pins and address of the touch panel inside an ST77922 display.
#[derive(Clone, Copy, Debug)]
pub struct St77922TouchPins {
    /// The I2C data line.
    pub sda: i32,
    /// The I2C clock line.
    pub scl: i32,
    /// The reset line, which a low level resets.
    pub reset: i32,
    /// The interrupt line, which goes low when the panel has a sample.
    pub interrupt: i32,
    /// The 7-bit I2C address.
    pub address: u8,
}

const I2C_CLOCK_HZ: u32 = 400_000;
const I2C_TIMEOUT_MS: u64 = 20;
const RESET_MS: u32 = 100;
const FIRMWARE_VERSION_REGISTER: u16 = 0x0000;
/// The registers from the largest X to the touch count, five bytes.
const LIMITS_REGISTER: u16 = 0x0005;
/// The number of fingers the controller tracks.
const MAX_TOUCHES_REGISTER: u16 = 0x0009;
const INFO_REGISTER: u16 = 0x0010;
/// The reports of the fingers, one after another.
const REPORT_REGISTER: u16 = 0x0014;
/// A finger's report: flags and the high bits of X, the low bits of X, the
/// two bytes of Y, the touched area, the intensity and a reserved byte.
const REPORT_BYTES: usize = 7;
const MAX_FINGERS: usize = 10;
/// The info bit that says the report registers hold coordinates.
const INFO_WITH_COORDINATES: u8 = 1 << 3;
/// The report bit that says its finger touches the panel.
const REPORT_VALID: u8 = 1 << 7;

/// How long a finger stays down without a new report from the controller.
const REPORT_TIMEOUT: Duration = Duration::from_millis(150);

/// The capacitive touch panel that the ST77922 display controller has built
/// in, on an I2C bus with 16-bit register addresses.
pub struct St77922Touch {
    bus: I2cDriver<'static>,
    address: u8,
    interrupt: Interrupt,
    /// Where the last report put the finger.
    finger: Option<(u16, u16)>,
    /// When the last report came.
    reported: Instant,
}

impl St77922Touch {
    /// Opens the panel on `pins`: resets it, then logs the firmware version
    /// and the limits it reports. The panel takes the chip's first I2C
    /// controller and its two pins for itself. The bus is `esp-idf-hal`'s
    /// driver, which an application's own I2C devices use as well: ESP-IDF
    /// aborts at start-up when its old and new I2C drivers are both linked.
    pub fn new(pins: St77922TouchPins) -> Result<Self, EspError> {
        reset(pins.reset)?;
        let config = I2cConfig::new()
            .baudrate(Hertz(I2C_CLOCK_HZ))
            .sda_enable_pullup(true)
            .scl_enable_pullup(true);
        // SAFETY: the board hands these peripherals to the panel alone.
        let bus = unsafe {
            I2cDriver::new(I2C0::steal(), io_pin(pins.sda)?, io_pin(pins.scl)?, &config)
        }?;
        let mut touch = Self {
            bus,
            address: pins.address,
            interrupt: Interrupt::new(pins.interrupt),
            finger: None,
            reported: Instant::now(),
        };
        let mut version = [0u8; 1];
        touch.read_registers(FIRMWARE_VERSION_REGISTER, &mut version)?;
        let mut limits = [0u8; 5];
        touch.read_registers(LIMITS_REGISTER, &mut limits)?;
        log::info!(
            "touch panel firmware {}, largest X {}, largest Y {}, {} fingers",
            version[0],
            u16::from_be_bytes([limits[0], limits[1]]),
            u16::from_be_bytes([limits[2], limits[3]]),
            limits[4],
        );
        Ok(touch)
    }

    fn read_registers(&mut self, register: u16, buffer: &mut [u8]) -> Result<(), EspError> {
        self.bus.write_read(
            self.address,
            &register.to_be_bytes(),
            buffer,
            TickType::new_millis(I2C_TIMEOUT_MS).ticks(),
        )
    }
}

/// The pin `number` of the chip, taken for the panel.
///
/// # Safety
///
/// Nothing else may use the pin.
unsafe fn io_pin(number: i32) -> Result<AnyIOPin<'static>, EspError> {
    let number = u8::try_from(number)
        .map_err(|_| EspError::from_infallible::<{ sys::ESP_ERR_INVALID_ARG }>())?;
    Ok(unsafe { AnyIOPin::steal(number) })
}

fn reset(pin: i32) -> Result<(), EspError> {
    esp!(unsafe { sys::gpio_reset_pin(pin) })?;
    esp!(unsafe { sys::gpio_set_direction(pin, sys::gpio_mode_t_GPIO_MODE_OUTPUT) })?;
    esp!(unsafe { sys::gpio_set_level(pin, 0) })?;
    FreeRtos::delay_ms(RESET_MS);
    esp!(unsafe { sys::gpio_set_level(pin, 1) })?;
    FreeRtos::delay_ms(RESET_MS);
    Ok(())
}

impl Touch for St77922Touch {
    fn arm<F>(&mut self, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        self.interrupt.arm(true, wake)
    }

    /// A read between two reports finds no coordinates, which is no news:
    /// the finger stays where the last report put it, and only a report
    /// without a valid finger lifts it. Taking the gap for a lift made one
    /// touch end twice, once at the gap and again at the next report.
    fn read(&mut self) -> Result<Option<(u16, u16)>, EspError> {
        let mut info = [0u8; 1];
        self.read_registers(INFO_REGISTER, &mut info)?;
        if info[0] & INFO_WITH_COORDINATES == 0 {
            if self.reported.elapsed() > REPORT_TIMEOUT {
                self.finger = None;
            }
            return Ok(self.finger);
        }
        // The controller keeps its interrupt line low until the host has read
        // the reports of all its fingers.
        let mut count = [0u8; 1];
        self.read_registers(MAX_TOUCHES_REGISTER, &mut count)?;
        let mut reports = [0u8; REPORT_BYTES * MAX_FINGERS];
        let reports = &mut reports[..REPORT_BYTES * usize::from(count[0]).min(MAX_FINGERS)];
        self.read_registers(REPORT_REGISTER, reports)?;
        self.finger = reports.as_chunks::<REPORT_BYTES>().0.iter().find_map(
            |&[flags, x_low, y_high, y_low, ..]| {
                (flags & REPORT_VALID != 0).then(|| {
                    (
                        u16::from(flags & 0x3F) << 8 | u16::from(x_low),
                        u16::from(y_high) << 8 | u16::from(y_low),
                    )
                })
            },
        );
        self.reported = Instant::now();
        Ok(self.finger)
    }
}
