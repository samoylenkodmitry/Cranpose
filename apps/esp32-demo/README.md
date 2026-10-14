# Cranpose ESP32-S3 Demo

A Cranpose screen on an ESP32-S3 under ESP-IDF: a title, a bar that an
infinite transition sweeps, and a counter that a coroutine `delay` advances
once a second. It draws on QEMU's virtual RGB panel at 320 by 240 pixels.

```sh
just esp32-setup        # once: Xtensa Rust, espflash, ldproxy, Espressif QEMU
just esp32-qemu         # shows the demo in a QEMU window
just esp32-qemu-check   # boots it headless and saves target/esp32/qemu.png
```

This directory is its own cargo workspace with the ESP-IDF configuration:
`.cargo/config.toml` selects `xtensa-esp32s3-espidf`, `rust-toolchain.toml`
the `esp-1.98.1` toolchain, `sdkconfig.defaults` the ESP-IDF settings, and
`partitions.csv` the flash layout. The workspace also holds
[`cranpose-platform-esp-idf`](../../crates/cranpose-platform/esp-idf), whose
README explains the runtime and the toolchain traps.

`sdkconfig.defaults` expects a module with 8 MB of octal PSRAM and 8 MB of
flash, such as N8R8 or N16R8. A board with quad PSRAM needs
`CONFIG_SPIRAM_MODE_QUAD=y` instead.
