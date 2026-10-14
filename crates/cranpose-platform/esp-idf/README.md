# Cranpose Platform ESP-IDF

`cranpose-platform-esp-idf` runs a Cranpose application on an ESP32-S3 display
under ESP-IDF. [`run`](src/lib.rs) composes the application into a
`cranpose::framebuffer::Framebuffer`. For each new picture the pixels renderer
draws only the areas that change, in strips of a few rows; each strip goes to
the panel as RGB565 through a buffer of internal DMA memory. No whole frame is
kept in memory. Between frames the task sleeps until the next frame, the next
timed event or work from another thread.

```rust
use cranpose_platform_esp_idf::{DisplayConfig, Panel, RunError, run};

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let Err(error) = Panel::qemu(320, 240)
        .map_err(RunError::from)
        .and_then(|panel| run(panel, DisplayConfig::default(), app::Screen));
    log::error!("stopped: {error}");
}
```

The crate belongs to the workspace of
[`apps/esp32-demo`](../../../apps/esp32-demo), not to the main one: ESP-IDF's
crates build only for ESP-IDF targets. The demo is the reference application,
with the cargo configuration, `sdkconfig.defaults` and partition table an
application needs. The framework crates must build for a CPU without 64-bit
atomics; `just clippy-no-atomic64` checks that with the stable toolchain.

## Toolchain

```sh
just esp32-setup        # Xtensa Rust, espflash, ldproxy, Espressif QEMU
just esp32-build        # the demo firmware; the first build downloads ESP-IDF
just esp32-clippy       # lints this crate and the demo for the ESP32-S3
just esp32-qemu-check   # boots the demo in QEMU and saves the panel picture
just esp32-qemu         # shows the demo on QEMU's virtual panel in a window
```

The demo firmware takes about 2.6 MB of flash: 2.2 MB of code and 0.35 MB of
read-only data. It turns off `cranpose`'s default features and passes an 18 KB
Latin subset of Noto Sans in `DisplayConfig::fonts`; the embedded default font
would add 1.3 MB.

The toolchain is `esp-1.98.1`. The `std` of esp 1.99.0.0 does not build for
ESP-IDF: it calls `libc::fchmodat`, which its `libc` does not define there.

The application's `Cargo.toml` keeps host build tools unstripped
(`[profile.*.build-override] strip = false`). The repository sets
`MACOSX_DEPLOYMENT_TARGET=12.0`, and macOS 27 refuses to load proc-macros that
the esp toolchain links for it and then strips.

## Configuration an application needs

- `CONFIG_ESP_MAIN_TASK_STACK_SIZE` of at least 32 KB: composition and layout
  run on the task that calls `run`.
- `CONFIG_FREERTOS_HZ=1000`: timed waits round to the tick, so at 100 Hz a
  16.7 ms frame interval waits 20 ms.
- Drawing takes a 30 KB RGBA strip and a 15 KB RGB565 strip in internal DMA
  memory, or one display row of each when the display is wider than 320
  pixels. The composition's own heap decides whether the app needs PSRAM.

## QEMU

Espressif's QEMU emulates the ESP32-S3 with PSRAM and a virtual RGB panel that
`Panel::qemu` opens. [`scripts/esp32/qemu_run.py`](../../../scripts/esp32/qemu_run.py)
builds the flash image, boots it with an eFuse image of a calibrated chip,
streams the serial log and stops at a given log line or a panic. Without ADC
calibration data in eFuse, ESP-IDF calibrates the ADC at startup and waits for
an ADC QEMU does not emulate.

QEMU runs the code with no cycle timing, so frame times measured there say
nothing about the chip.

## Status

- `Panel::qemu` is the only panel. Real panels (SPI, i80, RGB) go through the
  same `esp_lcd` interface and come with the first board.
- No touch input yet. `cranpose::framebuffer::Framebuffer::touch` takes the
  samples a touch controller driver reads.
