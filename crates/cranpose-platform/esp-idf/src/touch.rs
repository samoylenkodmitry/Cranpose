#![expect(unsafe_code)]

mod st77922;
mod xpt2046;

use std::{ffi::c_void, ptr};

use esp_idf_svc::{
    hal::task::do_yield,
    sys::{self, EspError, esp},
};
pub use st77922::{St77922Touch, St77922TouchPins};
pub use xpt2046::{Xpt2046Calibration, Xpt2046Pins, Xpt2046Touch};

/// A touch panel that [`run`](crate::run) reads.
pub trait Touch {
    /// Registers `wake`, which the panel's interrupt calls when a touch
    /// starts or moves. It returns whether the interrupt must yield to a task
    /// that it woke.
    fn arm<F>(&mut self, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static;

    /// Returns the position of the first finger in display pixels, or
    /// `None` while no finger touches the panel.
    fn read(&mut self) -> Result<Option<(u16, u16)>, EspError>;
}

/// A display without touch input.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoTouch;

impl Touch for NoTouch {
    fn arm<F>(&mut self, _wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        Ok(())
    }

    fn read(&mut self) -> Result<Option<(u16, u16)>, EspError> {
        Ok(None)
    }
}

type Wake = Box<dyn Fn() -> bool + Send + Sync>;

/// The line on which a touch controller signals a sample: it goes low.
struct Interrupt {
    pin: i32,
    wake: Option<Box<Wake>>,
}

unsafe extern "C" fn interrupt_handler(wake: *mut c_void) {
    let wake = unsafe { &*wake.cast::<Wake>() };
    if wake() {
        do_yield();
    }
}

impl Interrupt {
    fn new(pin: i32) -> Self {
        Self { pin, wake: None }
    }

    /// Calls `wake` from the interrupt at each falling edge. `pull_up` turns
    /// on the pin's own pull-up resistor, which the input-only pins lack.
    fn arm<F>(&mut self, pull_up: bool, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        let config = sys::gpio_config_t {
            pin_bit_mask: 1 << self.pin,
            mode: sys::gpio_mode_t_GPIO_MODE_INPUT,
            pull_up_en: if pull_up {
                sys::gpio_pullup_t_GPIO_PULLUP_ENABLE
            } else {
                sys::gpio_pullup_t_GPIO_PULLUP_DISABLE
            },
            pull_down_en: sys::gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: sys::gpio_int_type_t_GPIO_INTR_NEGEDGE,
        };
        esp!(unsafe { sys::gpio_config(&config) })?;
        // Another driver may have installed the shared ISR service.
        match esp!(unsafe { sys::gpio_install_isr_service(0) }) {
            Err(error) if error.code() != sys::ESP_ERR_INVALID_STATE => return Err(error),
            _ => {}
        }
        let wake: Box<Wake> = Box::new(Box::new(wake));
        esp!(unsafe {
            sys::gpio_isr_handler_add(
                self.pin,
                Some(interrupt_handler),
                ptr::from_ref(&*wake).cast_mut().cast(),
            )
        })?;
        self.wake = Some(wake);
        Ok(())
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        if self.wake.is_some()
            && let Err(error) = esp!(unsafe { sys::gpio_isr_handler_remove(self.pin) })
        {
            log::warn!("removing the touch interrupt failed: {error}");
        }
    }
}
