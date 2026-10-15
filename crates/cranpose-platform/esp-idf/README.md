# Cranpose Platform ESP-IDF

`cranpose-platform-esp-idf` runs a Cranpose application on an ESP32-S3 display
under ESP-IDF. [`run`](src/lib.rs) composes the application into a
`cranpose::framebuffer::Framebuffer`. For each new picture the pixels renderer
draws only the areas that change, in strips of a few rows; each strip goes to
the panel as RGB565 through a buffer of internal DMA memory. No whole frame is
kept in memory. Between frames the task sleeps until the next frame, the next
timed event or work from another thread.

[Cranpose on an emulated ESP32-S3](https://samoylenkodmitry.github.io/cranpose-esp32/)
runs firmware built on this crate in the browser, on Espressif's QEMU compiled
to WebAssembly. Its source is
[cranpose-esp32](https://github.com/samoylenkodmitry/cranpose-esp32), and the
emulator's is
[cranpose-esp32-qemu](https://github.com/samoylenkodmitry/cranpose-esp32-qemu).

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

The demo firmware takes about 2.7 MB of flash: 2.4 MB of code at
`opt-level = 2` with fat LTO in one codegen unit, and 0.3 MB of read-only data. It turns off `cranpose`'s default features and passes an 18 KB
Latin subset of Noto Sans in `DisplayConfig::fonts`; the embedded default font
would add 1.3 MB.

The toolchain is `esp-1.98.1`. The `std` of esp 1.99.0.0 does not build for
ESP-IDF: it calls `libc::fchmodat`, which its `libc` does not define there.

An application's release profile takes the demo's `opt-level = 2`,
`lto = "fat"` and `codegen-units = 1`: the measurements behind them are in
[`apps/esp32-demo/Cargo.toml`](../../../apps/esp32-demo/Cargo.toml).

The application's `Cargo.toml` keeps host build tools unstripped
(`[profile.*.build-override] strip = false`). The repository sets
`MACOSX_DEPLOYMENT_TARGET=12.0`, and macOS 27 refuses to load proc-macros that
the esp toolchain links for it and then strips.

## Configuration an application needs

- `CONFIG_ESP_MAIN_TASK_STACK_SIZE` of at least 32 KB: composition and layout
  run on the task that calls `run`.
- Drawing takes a 30 KB RGBA strip and a 15 KB RGB565 strip in internal DMA
  memory, or one display row of each when the display is wider than 320
  pixels. The composition's own heap decides whether the app needs PSRAM.

## Thread-local lookups

Rust's `std` keeps each `thread_local!` of an ESP-IDF build in a pthread key,
and ESP-IDF's `pthread_getspecific` masks interrupts to find the current task
and walks the task's list of values: about 90 instructions for every access,
some 190 of them a frame in the demo. The crate's
[`cranpose_fast_tls`](components/cranpose_fast_tls/fast_tls.c) component,
which ESP-IDF builds into every application that depends on the crate, wraps
`pthread_getspecific` and `pthread_setspecific`: each task keeps a copy of its
values of the first 32 keys in a C thread-local array, read in a few
instructions through the `THREADPTR` register. The array takes 128 bytes of
every task's stack.

## Allocator

ESP-IDF's heap takes a spinlock for each call, and Rust's 8-byte aligned
blocks go through `heap_caps_aligned_alloc`: about 1,000 instructions for an
allocation and its free. With the default `small-block-alloc` feature the
crate installs [`SmallBlockAllocator`](src/small_alloc.rs) as the global
allocator. Each task keeps up to 8 freed blocks of each of ten size classes
up to 128 bytes in lists of its own and takes them back without a lock;
other blocks come from the heap. A task's blocks go back to the heap when it
is deleted. An application that installs another global allocator, or links
code that frees memory from C `malloc` through Rust's allocator, turns the
feature off with `default-features = false`.

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
