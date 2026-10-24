use std::convert::Infallible;

use esp_idf_svc::sys::EspError;

use crate::{DisplayConfig, NoTouch, Panel, RunError, Touch, run};

/// A board: its display, its touch panel and the display settings that suit
/// the chip behind them. An application names the board it runs on and
/// leaves the pins, the controller setup and the calibration to it.
pub trait Board {
    /// The touch panel of the board; [`NoTouch`] for a board without one.
    type Touch: Touch;

    /// Opens the display and the touch panel.
    fn open() -> Result<(Panel, Self::Touch), EspError>;

    /// The settings that suit the board, without fonts.
    fn display_config() -> DisplayConfig {
        DisplayConfig::default()
    }
}

/// Composes `content` on the display of board `B` and drives it until an
/// error stops it, with `fonts` for its text. See [`run`].
pub fn run_board<B: Board>(
    fonts: &'static [&'static [u8]],
    content: impl FnMut() + 'static,
) -> Result<Infallible, RunError> {
    let (panel, touch) = B::open()?;
    let config = DisplayConfig {
        fonts,
        ..B::display_config()
    };
    run(panel, touch, config, content)
}

/// QEMU's virtual 320 by 240 panel, shown by `idf.py qemu --graphics`. It
/// has no touch. Real hardware has no such panel and fails to open it.
#[derive(Clone, Copy, Debug)]
pub struct Qemu;

impl Board for Qemu {
    type Touch = NoTouch;

    fn open() -> Result<(Panel, NoTouch), EspError> {
        Ok((Panel::qemu(320, 240)?, NoTouch))
    }
}
