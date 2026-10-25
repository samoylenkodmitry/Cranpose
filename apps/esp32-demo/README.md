# Cranpose ESP32 Demo

A Cranpose screen on an ESP32-S3 or ESP32 under ESP-IDF: a title, a bar that
an infinite transition sweeps, a counter that a coroutine `delay` advances
once a second and a box that counts taps. Without a board feature it draws on
QEMU's virtual RGB panel at 320 by 480 pixels, the size of the boards'
panels.

```sh
just esp32-setup        # once: Xtensa Rust, espflash, ldproxy, Espressif QEMU
just esp32-qemu         # shows the demo in a QEMU window
just esp32-qemu-check   # boots it headless and saves target/esp32/qemu.png
```

Two boards run it on their own display and touch panel, each with a cargo
feature:

```sh
just esp32-flash-es3c35p /dev/cu.usbmodem31301    # Hosyond ES3C35P, ESP32-S3
just esp32-flash-e32r35t /dev/cu.usbserial-3120   # 3.5-inch ESP32-32E, ESP32
```

| Feature | Board | Chip | Display | Touch |
| --- | --- | --- | --- | --- |
| `es3c35p` | Hosyond ES3C35P | ESP32-S3 N16R8 | 320 by 480 ST77922 on quad SPI | capacitive, built into the display controller, I2C address 0x55 |
| `e32r35t` | LCDWiki E32R35T | ESP32-D0WD-V3, 4 MB flash, no PSRAM | 320 by 480 ST7796 on SPI | resistive XPT2046 on the display's bus |

Each board is a module in [`src/board`](src/board): its pins, bus clock,
controller setup, touch calibration and strip size, wired to the drivers of
[`cranpose-platform-esp-idf`](../../crates/cranpose-platform/esp-idf). A new
board with a MIPI DCS display controller on SPI is one more module like
[`e32r35t.rs`](src/board/e32r35t.rs). The board vendors' start-up sequences
also hold the commands that Espressif's controller drivers sent around them,
so the bus carries the same bytes as with those drivers.

This directory is its own cargo workspace with the ESP-IDF configuration:
`.cargo/config.toml` selects `xtensa-esp32s3-espidf`, `rust-toolchain.toml`
the `esp-1.98.1` toolchain, `sdkconfig.defaults` the ESP-IDF settings, and
`partitions.csv` the flash layout. The workspace also holds
[`cranpose-platform-esp-idf`](../../crates/cranpose-platform/esp-idf), whose
README explains the runtime and the toolchain traps.

`sdkconfig.defaults` holds the settings that both chips share.
`sdkconfig.defaults.esp32s3` and `sdkconfig.defaults.esp32` hold the settings
of each chip, which ESP-IDF adds by the chip it builds for: the S3 file
expects a module with 8 MB of octal PSRAM, such as N8R8 or N16R8 (a board with
quad PSRAM needs `CONFIG_SPIRAM_MODE_QUAD=y` instead). `sdkconfig.es3c35p` and
`sdkconfig.e32r35t` hold the settings of each board: console, flash size,
flash in QIO mode, which the bootloader that ESP-IDF built turns on, so the
`just` recipes flash it with the app.
`partitions_4mb.csv` fits the classic board's 4 MB of flash. The classic ESP32
builds for another target, which needs its own `MCU`:
`just esp32-build-e32r35t` runs
`ESP_IDF_SDKCONFIG_DEFAULTS="sdkconfig.defaults;sdkconfig.e32r35t" MCU=esp32
cargo build --release --target xtensa-esp32-espidf --features e32r35t`. The platform crate's README explains
what these settings do to a frame.
