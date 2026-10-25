#![expect(unsafe_code, reason = "calls into ESP-IDF and its drivers")]

mod st77922;
mod xpt2046;

use std::{ffi::c_void, ptr, time::Instant};

use esp_idf_svc::{
    hal::{
        gpio::{AnyInputPin, Pin},
        task::do_yield,
    },
    sys::{self, EspError, esp},
};
pub use st77922::St77922Touch;
pub use xpt2046::{Xpt2046Calibration, Xpt2046Touch};

/// A touch panel that [`run`](crate::run) reads.
pub trait Touch {
    /// Registers `wake`, which the panel's interrupt calls when a touch
    /// starts or moves. It returns whether the interrupt must yield to a task
    /// that it woke.
    fn arm<F>(&mut self, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
        Self: Sized;

    /// Returns the position of the first finger in display pixels, or
    /// `None` while no finger touches the panel or its setup runs.
    fn read(&mut self) -> Result<Option<(u16, u16)>, EspError>;

    /// Runs the steps of the panel's setup whose wait has passed at `now`,
    /// and returns when the next one is due: `None` once the panel is set
    /// up. [`run`](crate::run) calls it while it shows the first pictures,
    /// so the waits of a reset do not hold the first one back.
    fn set_up(&mut self, now: Instant) -> Result<Option<Instant>, EspError> {
        let _ = now;
        Ok(None)
    }
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

/// How the axes of a touch controller lie on the display. The default puts
/// the controller's X along the display's width and its Y along the height,
/// both growing from the top left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orientation {
    /// The controller's Y axis runs along the display's width.
    pub swap_xy: bool,
    /// Display X grows from the right edge.
    pub mirror_x: bool,
    /// Display Y grows from the bottom edge.
    pub mirror_y: bool,
}

impl Orientation {
    /// Places `point` of a controller whose axes span `size` pixels on the
    /// display.
    fn place(self, point: (u16, u16), size: (u16, u16)) -> (u16, u16) {
        let ((x, y), (width, height)) = if self.swap_xy {
            ((point.1, point.0), (size.1, size.0))
        } else {
            (point, size)
        };
        let mirror = |value: u16, size: u16, mirrored: bool| {
            let last = size.saturating_sub(1);
            let value = value.min(last);
            if mirrored { last - value } else { value }
        };
        (
            mirror(x, width, self.mirror_x),
            mirror(y, height, self.mirror_y),
        )
    }
}

type Wake = Box<dyn Fn() -> bool + Send + Sync>;

/// The line on which a touch controller signals a sample: it goes low.
struct Interrupt<'d> {
    pin: AnyInputPin<'d>,
    wake: Option<Box<Wake>>,
}

unsafe extern "C" fn interrupt_handler(wake: *mut c_void) {
    // SAFETY: the handler's argument is the `Wake` box of the `Interrupt`
    // that added it, which removes the handler before it frees the box.
    let wake = unsafe { &*wake.cast::<Wake>() };
    if wake() {
        do_yield();
    }
}

impl<'d> Interrupt<'d> {
    fn new(pin: AnyInputPin<'d>) -> Self {
        Self { pin, wake: None }
    }

    /// Calls `wake` from the interrupt at each falling edge. `pull_up` turns
    /// on the pin's own pull-up resistor, which the input-only pins lack.
    fn arm<F>(&mut self, pull_up: bool, wake: F) -> Result<(), EspError>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        let pin = i32::from(self.pin.pin());
        let config = sys::gpio_config_t {
            pin_bit_mask: 1 << pin,
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
                pin,
                Some(interrupt_handler),
                ptr::from_ref(&*wake).cast_mut().cast(),
            )
        })?;
        self.wake = Some(wake);
        Ok(())
    }
}

impl Drop for Interrupt<'_> {
    fn drop(&mut self) {
        if self.wake.is_some()
            && let Err(error) =
                esp!(unsafe { sys::gpio_isr_handler_remove(i32::from(self.pin.pin())) })
        {
            log::warn!("removing the touch interrupt failed: {error}");
        }
    }
}
