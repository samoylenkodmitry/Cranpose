# Cranpose Platform ESP-IDF

`cranpose-platform-esp-idf` runs a Cranpose application on an ESP32 or
ESP32-S3 display under ESP-IDF. [`run`](src/lib.rs) composes the application into a
`cranpose::framebuffer::Framebuffer`. For each new picture the pixels renderer
draws only the areas that change, in strips of a few rows; each strip goes to
the panel as RGB565 through a buffer of internal DMA memory. No whole frame is
kept in memory. Between frames the task sleeps until the next frame, the next
timed event or work from another thread.

[Cranpose on an emulated ESP32-S3](https://samoylenkodmitry.github.io/cranpose-esp32/)
runs firmware built on this crate in the browser, on Espressif's QEMU compiled
to WebAssembly. Its source,
[cranpose-esp32](https://github.com/samoylenkodmitry/cranpose-esp32), is also
the template from which to start an ESP32 application. The emulator's source
is [cranpose-esp32-qemu](https://github.com/samoylenkodmitry/cranpose-esp32-qemu).

## What the crate does and what the application gives it

The crate holds the code that is the same on every board: the run loop, the
drawing pass, the display controller protocol, the touch controller
protocols, the allocator and the ESP-IDF shims below. The application holds
everything that belongs to its board and its chip: the pins, the bus and its
clock, the controller's start-up sequence, the touch calibration, the strip
size, `sdkconfig.defaults` and the partition table.

```rust
use std::rc::Rc;

use cranpose_platform_esp_idf::{
    Backlight, Controller, DisplayConfig, InitCommand, Lines, Panel, SpiWiring, run,
};
use esp_idf_svc::hal::{
    gpio::Level,
    peripherals::Peripherals,
    spi::{Dma, SpiDriver, SpiDriverConfig, config::MODE_0},
    units::Hertz,
};

static FONTS: &[&[u8]] = &[include_bytes!("../assets/NotoSans-Latin.ttf")];

const INIT: &[InitCommand] = &[
    InitCommand::new(0x11, &[], 120), // sleep out
    InitCommand::new(0x3A, &[0x55], 0), // 16-bit colour
    InitCommand::new(0x36, &[0x48], 0), // scan direction and colour order
    InitCommand::new(0x29, &[], 0),   // display on
];

fn main() -> Result<(), cranpose_platform_esp_idf::RunError> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;
    let config = DisplayConfig {
        fonts: FONTS,
        ..DisplayConfig::default()
    };
    let strip_bytes = 320 * config.strip_rows as usize * 2;
    let bus = SpiDriver::new(
        peripherals.spi2,
        pins.gpio14,
        pins.gpio13,
        Some(pins.gpio12),
        &SpiDriverConfig::new().dma(Dma::Auto(strip_bytes)),
    )?;
    let wiring = SpiWiring {
        cs: pins.gpio15.into(),
        lines: Lines::Single { dc: pins.gpio2.into() },
        clock: Hertz(40_000_000),
        mode: MODE_0,
        reset: None,
        backlight: Some(Backlight { pin: pins.gpio27.into(), on: Level::High }),
    };
    let panel = Panel::spi(bus, wiring, Controller::new(320, 480, INIT))?;
    let Err(error) = run(panel, cranpose_platform_esp_idf::NoTouch, config, app::Screen);
    Err(error)
}
```

[`apps/esp32-demo/src/board`](../../../apps/esp32-demo/src/board) wires two
boards and QEMU this way, with touch.

### Displays

- `Panel::spi` drives a display controller that takes MIPI DCS commands, such
  as the ST7789, ST7796, ILI9341 or ST77922, over `esp_lcd`'s SPI panel IO.
  `Lines::Single` is a bus with a data/command line, `Lines::Quad` a quad SPI
  bus whose commands go behind the opcodes 0x02 and 0x32. `Controller` gives
  the size, the offset of the visible area in the controller's memory, the
  column and row alignment of update windows (the ST77922 takes windows on
  multiples of 4 columns only) and the start-up sequence, which comes from
  the glass vendor. Without a reset line the panel sends the software reset
  command. The backlight turns on once the first picture is on the display.
- `Panel::qemu` opens QEMU's virtual RGB panel.
- A display's colour transfer is asynchronous: the panel reads a strip by DMA
  after the draw call returns. The presenter keeps two strips in turn and
  waits on the transfer-done callback before it refills one.
- Give the bus a DMA transfer size of at least one strip,
  `strip_rows * width * 2` bytes. A smaller one splits each strip into
  several transfers.

### Touch

`run` reads a `Touch`: the first finger goes to the composition as down, move
and up. An interrupt on the controller's line wakes the loop, which reads the
panel at each frame while a finger is down. `NoTouch` stands for a display
without one.

- `Xpt2046Touch` reads a resistive XPT2046 through an `esp-idf-hal`
  `SpiDeviceDriver`, often on the display's bus. `Xpt2046Calibration` maps its
  raw readings to pixels and holds the pressure threshold.
- `St77922Touch` reads the capacitive panel built into the ST77922 through an
  `esp-idf-hal` `I2cDriver`. ESP-IDF 5.5 aborts at start-up when its old and
  new I2C drivers are both linked; `esp-idf-hal` uses the old one, so the
  application's other I2C devices go through `I2cDriver` too. The controller
  holds its interrupt line low until the host has read the reports of all its
  fingers. A read without coordinates keeps the finger down; only a report
  without a valid finger lifts it.
- `Orientation` turns a controller's axes onto the display, for a glass
  mounted in another direction.

### Strips and memory

`DisplayConfig::strip_rows` sets the rows of a strip. A strip takes 10 bytes
of internal memory a pixel: 4 for its RGBA drawing and 2 for each of the two
RGB565 buffers the panel reads by DMA. The default, 8 rows, takes 25 KB on a
320-pixel wide display. Taller strips cost fewer draw and transfer calls.

## Run loop

The calling task becomes the UI thread. Composition and layout run on its
stack, so give it at least 32 KB (`CONFIG_ESP_MAIN_TASK_STACK_SIZE` for the
main task). An `esp_timer` wakes the task at each frame or timed event, so
frame timing does not depend on `CONFIG_FREERTOS_HZ`.

While frames take longer than the frame interval the loop has nothing to wait
for. A loop that never blocks starves the idle task of its core, and ESP-IDF's
task watchdog reports it every 5 s. The loop therefore sleeps one FreeRTOS
tick when it has not slept for 500 ms. On the classic ESP32 demo, whose frames
take 20 ms, this costs about one frame in 5 s.

The crate belongs to the workspace of
[`apps/esp32-demo`](../../../apps/esp32-demo), not to the main one: ESP-IDF's
crates build only for ESP-IDF targets. The framework crates must build for a
CPU without 64-bit atomics; `just clippy-no-atomic64` checks that with the
stable toolchain.

## Code fetch

A frame runs through several hundred KB of framework code, more than the
32 KB instruction cache holds, so on both chips most of a frame's cycles wait
for code: 530K of a 620K-cycle frame of the demo on the ESP32-S3 before the
settings below, which apply to every app and live in the demo's sdkconfig
files and in the template's.

| Setting | Where | Effect on the demo's frame |
| --- | --- | --- |
| Flash in QIO mode at 80 MHz, flashed with the bootloader ESP-IDF built (espflash's own does not turn QIO on) | the board's sdkconfig | ES3C35P 10.2 to 6.1 ms, E32R35T 20.4 to 6.4 ms |
| 32 KB instruction and 64 KB data cache, 64-byte data lines | `sdkconfig.defaults.esp32s3` | ES3C35P 6.1 to 5.7 ms |
| Code and read-only data copied to octal PSRAM at start (`CONFIG_SPIRAM_XIP_FROM_PSRAM`) | `sdkconfig.defaults.esp32s3` | ES3C35P 5.7 to 2.7 ms, for about 200 ms of copying at start |
| The framework's per-frame code in IRAM | this crate's [`cranpose_hot_code`](components/cranpose_hot_code/README.md) component, on by default | ES3C35P 2.7 to 1.4 ms, E32R35T 6.4 to 4.5 ms |
| No check of the app image at boot | `sdkconfig.defaults` | the first frame 470 ms (ES3C35P) and 830 ms (E32R35T) sooner |

[`benchmarks/esp32`](../../../benchmarks/esp32) compares the demo with the
same screen in LVGL on both boards, and `scripts/esp32` holds the tools that
measured these: a sampling profiler that runs on the chip, and a script that
builds, flashes and logs an app with it or with the Xtensa performance
counters.

## Toolchain

```sh
just esp32-setup        # Xtensa Rust, espflash, ldproxy, Espressif QEMU
just esp32-build        # the demo firmware; the first build downloads ESP-IDF
just esp32-clippy       # lints this crate and the demo for the ESP32-S3
just esp32-flash-es3c35p /dev/cu.usbmodem31301     # builds, flashes with ESP-IDF's bootloader and shows the log
just esp32-flash-e32r35t /dev/cu.usbserial-3120
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

## Memory of a chip without PSRAM

The demo's composition takes about 150 KB of heap, and a text takes four
bytes a pixel of its rectangle while it draws. A chip without PSRAM, such as
the classic ESP32 with about 300 KB of heap, runs the demo with
`CONFIG_FREERTOS_UNICORE=y` (the second core's idle and IPC tasks cost
20 KB) and 8-row strips. The main task's 32 KB stack suits both chips: the
demo's deepest use of it was about 22 KB.

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
