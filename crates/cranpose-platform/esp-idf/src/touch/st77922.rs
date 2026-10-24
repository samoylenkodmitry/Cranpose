#![expect(unsafe_code)]

use std::{
    mem::zeroed,
    ptr,
    time::{Duration, Instant},
};

use esp_idf_svc::{
    hal::delay::FreeRtos,
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
    pub address: u16,
}

const I2C_CLOCK_HZ: u32 = 400_000;
const I2C_TIMEOUT_MS: i32 = 20;
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
    bus: sys::i2c_master_bus_handle_t,
    device: sys::i2c_master_dev_handle_t,
    interrupt: Interrupt,
    /// Where the last report put the finger.
    finger: Option<(u16, u16)>,
    /// When the last report came.
    reported: Instant,
}

impl St77922Touch {
    /// Opens the panel on `pins`: resets it, then logs the firmware version
    /// and the limits it reports.
    pub fn new(pins: St77922TouchPins) -> Result<Self, EspError> {
        reset(pins.reset)?;
        let mut bus_config: sys::i2c_master_bus_config_t = unsafe { zeroed() };
        bus_config.i2c_port = -1;
        bus_config.sda_io_num = pins.sda;
        bus_config.scl_io_num = pins.scl;
        bus_config.__bindgen_anon_1.clk_source = sys::soc_periph_i2c_clk_src_t_I2C_CLK_SRC_DEFAULT;
        bus_config.glitch_ignore_cnt = 7;
        bus_config.flags.set_enable_internal_pullup(1);
        let mut bus: sys::i2c_master_bus_handle_t = ptr::null_mut();
        esp!(unsafe { sys::i2c_new_master_bus(&bus_config, &mut bus) })?;
        let mut touch = Self {
            bus,
            device: ptr::null_mut(),
            interrupt: Interrupt::new(pins.interrupt),
            finger: None,
            reported: Instant::now(),
        };
        let mut device_config: sys::i2c_device_config_t = unsafe { zeroed() };
        device_config.dev_addr_length = sys::i2c_addr_bit_len_t_I2C_ADDR_BIT_LEN_7;
        device_config.device_address = pins.address;
        device_config.scl_speed_hz = I2C_CLOCK_HZ;
        esp!(unsafe {
            sys::i2c_master_bus_add_device(touch.bus, &device_config, &mut touch.device)
        })?;
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
        let address = register.to_be_bytes();
        esp!(unsafe {
            sys::i2c_master_transmit_receive(
                self.device,
                address.as_ptr(),
                address.len(),
                buffer.as_mut_ptr(),
                buffer.len(),
                I2C_TIMEOUT_MS,
            )
        })
    }
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

impl Drop for St77922Touch {
    fn drop(&mut self) {
        if !self.device.is_null()
            && let Err(error) = esp!(unsafe { sys::i2c_master_bus_rm_device(self.device) })
        {
            log::warn!("closing the touch panel failed: {error}");
        }
        if let Err(error) = esp!(unsafe { sys::i2c_del_master_bus(self.bus) }) {
            log::warn!("closing the I2C bus failed: {error}");
        }
    }
}
