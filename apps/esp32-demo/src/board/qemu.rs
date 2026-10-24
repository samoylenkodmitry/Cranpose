//! QEMU's virtual 320 by 240 panel, shown by `idf.py qemu --graphics`, on an
//! emulated ESP32-S3 with PSRAM. It has no touch.

use cranpose_platform_esp_idf::{DisplayConfig, NoTouch, Panel};
use esp_idf_svc::sys::EspError;

/// The demo's title.
pub const TITLE: &str = "Cranpose on ESP32-S3";

/// The display settings of the emulated chip.
pub fn config() -> DisplayConfig {
    DisplayConfig {
        strip_rows: 24,
        ..DisplayConfig::default()
    }
}

/// Opens the virtual panel.
pub fn open() -> Result<(Panel<'static>, NoTouch), EspError> {
    Ok((Panel::qemu(320, 240)?, NoTouch))
}
