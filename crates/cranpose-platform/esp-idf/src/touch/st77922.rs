use std::time::{Duration, Instant};

use esp_idf_svc::{
    hal::{
        delay::TickType,
        gpio::{self, AnyInputPin, AnyOutputPin, PinDriver},
        i2c::I2cDriver,
    },
    sys::EspError,
};

use super::{Interrupt, Orientation, Touch};

const I2C_TIMEOUT_MS: u64 = 20;
/// How long the reset line stays low, and how long the controller then takes
/// to start its firmware, as in the vendor's driver.
const RESET: Duration = Duration::from_millis(100);
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
pub struct St77922Touch<'d> {
    bus: I2cDriver<'d>,
    address: u8,
    interrupt: Interrupt<'d>,
    /// Held high, after the reset, while the panel is open.
    reset: PinDriver<'d, gpio::Output>,
    setup: Setup,
    orientation: Orientation,
    /// The pixels along the controller's X and Y axes, as it reports them.
    size: (u16, u16),
    /// Where the last report put the finger.
    finger: Option<(u16, u16)>,
    /// When the last report came.
    reported: Instant,
}

/// Where the controller is in its start-up.
#[derive(Clone, Copy)]
enum Setup {
    /// The reset line stays low until then.
    Resetting(Instant),
    /// The controller starts its firmware until then.
    Starting(Instant),
    Ready,
}

impl<'d> St77922Touch<'d> {
    /// Starts the reset of the panel through `reset`, to read it on `bus` at
    /// `address`, with its interrupt on `interrupt`. Once
    /// [`Touch::set_up`] has seen the reset through, it logs the firmware
    /// version and the limits the panel reports.
    ///
    /// ESP-IDF aborts at start-up when its old and new I2C drivers are both
    /// linked, so the bus is `esp-idf-hal`'s driver, which the application's
    /// other I2C devices use as well.
    pub fn new(
        bus: I2cDriver<'d>,
        address: u8,
        reset: impl Into<AnyOutputPin<'d>>,
        interrupt: impl Into<AnyInputPin<'d>>,
        orientation: Orientation,
    ) -> Result<Self, EspError> {
        let mut reset = PinDriver::output(reset.into())?;
        reset.set_low()?;
        let now = Instant::now();
        Ok(Self {
            bus,
            address,
            interrupt: Interrupt::new(interrupt.into()),
            reset,
            setup: Setup::Resetting(now + RESET),
            orientation,
            size: (u16::MAX, u16::MAX),
            finger: None,
            reported: now,
        })
    }

    /// Reads the firmware version and the limits of the started controller.
    fn read_limits(&mut self) -> Result<(), EspError> {
        let mut version = [0u8; 1];
        self.read_registers(FIRMWARE_VERSION_REGISTER, &mut version)?;
        let mut limits = [0u8; 5];
        self.read_registers(LIMITS_REGISTER, &mut limits)?;
        self.size = (
            u16::from_be_bytes([limits[0], limits[1]]),
            u16::from_be_bytes([limits[2], limits[3]]),
        );
        log::info!(
            "touch panel firmware {}, largest X {}, largest Y {}, {} fingers",
            version[0],
            self.size.0,
            self.size.1,
            limits[4],
        );
        Ok(())
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

impl Touch for St77922Touch<'_> {
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
        if self.set_up(Instant::now())?.is_some() {
            return Ok(None);
        }
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
        let (orientation, size) = (self.orientation, self.size);
        self.finger = reports.as_chunks::<REPORT_BYTES>().0.iter().find_map(
            |&[flags, x_low, y_high, y_low, ..]| {
                (flags & REPORT_VALID != 0).then(|| {
                    let point = (
                        u16::from(flags & 0x3F) << 8 | u16::from(x_low),
                        u16::from(y_high) << 8 | u16::from(y_low),
                    );
                    orientation.place(point, size)
                })
            },
        );
        self.reported = Instant::now();
        Ok(self.finger)
    }

    fn set_up(&mut self, now: Instant) -> Result<Option<Instant>, EspError> {
        match self.setup {
            Setup::Resetting(until) | Setup::Starting(until) if now < until => Ok(Some(until)),
            Setup::Resetting(_) => {
                self.reset.set_high()?;
                let until = now + RESET;
                self.setup = Setup::Starting(until);
                Ok(Some(until))
            }
            Setup::Starting(_) => {
                self.read_limits()?;
                self.setup = Setup::Ready;
                Ok(None)
            }
            Setup::Ready => Ok(None),
        }
    }
}
