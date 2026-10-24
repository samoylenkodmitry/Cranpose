# Per-frame code in IRAM

An ESP32 runs its code from flash or PSRAM through a cache: 32 KB of
instructions on the ESP32-S3, 32 KB for code and data together on the classic
ESP32. A frame of a Cranpose app runs through composition, layout, the scene,
the damage pass and the drawing pass, several hundred KB of code, so most of
each frame's code misses the cache. The Xtensa performance counters show it on
the demo's animated frame:

| ES3C35P (ESP32-S3), per frame | instructions | cycles waiting for code | cycles waiting for data |
| --- | --- | --- | --- |
| all code in PSRAM | 86K | 530K | 59K |
| this component on | 86K | 248K | 37K |

This component links the framework functions that a frame runs most into
IRAM, the internal RAM that the CPU fetches without the cache. A linker
script for each chip lists them, `esp32s3.ld` and `esp32.ld`, and inserts an
output section after ESP-IDF's own IRAM code.

| Demo frame | composition and layout | drawing and sending |
| --- | --- | --- |
| ES3C35P, without | 2.0 ms | 0.6 ms |
| ES3C35P, with 128 KB | 1.0 ms | 0.5 ms |
| E32R35T (classic ESP32), without | 5.1 ms | 1.3 ms |
| E32R35T, with 64 KB | 3.4 ms | 1.0 ms |

## Memory

- ESP32-S3: IRAM and the heap share the same internal RAM, so the 133 KB
  section (128 KB of functions and their literals) is that much less internal
  heap. With PSRAM, the heap takes larger blocks from PSRAM.
- Classic ESP32: the section takes 66 KB of IRAM, which the heap otherwise
  only lends to 32-bit allocations.

`CONFIG_CRANPOSE_HOT_CODE_IN_IRAM` (on by default, under "Cranpose" in
menuconfig) turns it off, for an application that needs the memory more.

## How the functions are chosen

`scripts/esp32/sampler` samples the PC 5000 times a second while the demo
runs, and `scripts/esp32/hot_code.py` picks the framework functions with the
most samples per byte until the budget is full. Functions of the profiled
app, and generic functions instantiated with its types, stay out: another app
has other ones. The names in the scripts are patterns of the v0 mangled names
in which crate hashes and back-references are wildcards
(`scripts/esp32/v0_pattern.py`), so they match the same functions in any
build and any app. A pattern that matches nothing costs nothing; a function
renamed since the profile runs from the cache again.

To profile a board and write its script again, after changes that move the
framework's per-frame work:

```sh
just esp32-profile es3c35p
python3 scripts/esp32/hot_code.py target/esp32/profile-es3c35p.elf target/esp32/profile-es3c35p.log 128 crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32s3.ld --board "apps/esp32-demo on the ES3C35P"
just esp32-profile e32r35t
python3 scripts/esp32/hot_code.py target/esp32/profile-e32r35t.elf target/esp32/profile-e32r35t.log 64 crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32.ld --board "apps/esp32-demo on the E32R35T"
```

The profile runs without this component (`scripts/esp32/sdkconfig.no_hot_code`),
so its samples show what the cache costs, and prints the hottest functions
and crates.
