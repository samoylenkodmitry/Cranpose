//! QEMU's virtual panel at 320 by 480, the size of the boards' panels, shown
//! by `idf.py qemu --graphics`, on an emulated ESP32-S3 with PSRAM or a
//! classic ESP32. It has no touch.

use cranpose_platform_esp_idf::{DisplayConfig, NoTouch, Panel};
use esp_idf_svc::sys::EspError;

/// The demo's title.
#[cfg(not(esp_idf_idf_target_esp32))]
pub const TITLE: &str = "Cranpose on ESP32-S3";
/// The demo's title.
#[cfg(esp_idf_idf_target_esp32)]
pub const TITLE: &str = "Cranpose on ESP32";

/// The rows of a strip, as the board of the chip takes them: the classic
/// ESP32 has no PSRAM and less internal memory.
#[cfg(not(esp_idf_idf_target_esp32))]
const STRIP_ROWS: u32 = 24;
#[cfg(esp_idf_idf_target_esp32)]
const STRIP_ROWS: u32 = 8;

/// The display settings of the emulated chip.
pub fn config() -> DisplayConfig {
    DisplayConfig {
        strip_rows: STRIP_ROWS,
        ..DisplayConfig::default()
    }
}

/// Opens the virtual panel.
pub fn open() -> Result<(Panel<'static>, NoTouch), EspError> {
    Ok((Panel::qemu(320, 480)?, NoTouch))
}
