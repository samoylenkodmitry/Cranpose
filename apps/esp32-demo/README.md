# Cranpose ESP32 Demo

A Cranpose screen on an ESP32-S3 or ESP32 under ESP-IDF: a title, a bar that
an infinite transition sweeps, a counter that a coroutine `delay` advances
once a second and a box that counts taps. Without a board feature it draws on
QEMU's virtual RGB panel at 320 by 240 pixels.

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

This directory is its own cargo workspace with the ESP-IDF configuration:
`.cargo/config.toml` selects `xtensa-esp32s3-espidf`, `rust-toolchain.toml`
the `esp-1.98.1` toolchain, `sdkconfig.defaults` the ESP-IDF settings, and
`partitions.csv` the flash layout. The workspace also holds
[`cranpose-platform-esp-idf`](../../crates/cranpose-platform/esp-idf), whose
README explains the runtime and the toolchain traps.

`sdkconfig.defaults` holds the settings that both chips share.
`sdkconfig.defaults.esp32s3` expects a module with 8 MB of octal PSRAM, such as N8R8 or N16R8; a board with
quad PSRAM needs `CONFIG_SPIRAM_MODE_QUAD=y` instead. `sdkconfig.es3c35p` adds
the ES3C35P's USB console and 16 MB of flash, and `sdkconfig.defaults.esp32`
the classic board's 4 MB of flash with `partitions_4mb.csv`.
