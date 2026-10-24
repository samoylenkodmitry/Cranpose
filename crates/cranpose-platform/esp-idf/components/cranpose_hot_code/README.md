# Per-frame code in IRAM

An ESP32 runs its code from flash or PSRAM through a cache: 32 KB of
instructions on the ESP32-S3, 32 KB for code and data together on the classic
ESP32. A frame of a Cranpose app runs through its animations, the scene, the
damage pass and the drawing pass, and through composition and layout when
they have work, more code than the cache holds, so most of each frame's code
misses the cache. The Xtensa performance counters show it on the demo's
animated frame:

| Per frame | instructions | cycles waiting for code | cycles waiting for data |
| --- | --- | --- | --- |
| ES3C35P (ESP32-S3), all code in flash | 64K | 714K | 81K |
| ES3C35P, this component on | 64K | 40K | 11K |
| E32R35T (classic ESP32), all code in flash | 55K | 781K | 103K |
| E32R35T, this component on | 56K | 306K | 51K |

This component links the framework functions that a frame runs most into
IRAM, the internal RAM that the CPU fetches without the cache. A linker
script for each chip lists them, `esp32s3.ld` and `esp32.ld`, and inserts an
output section after ESP-IDF's own IRAM code.

| Frame | demo | a drawn line of text changing every frame | a composed one |
| --- | --- | --- | --- |
| ES3C35P, without | 3.15 ms | 5.93 ms | 8.82 ms |
| ES3C35P, with 125 KB | 0.50 ms | 2.65 ms | 5.82 ms |
| E32R35T, without | 3.50 ms | 7.22 ms | 10.19 ms |
| E32R35T, with 64 KB | 1.60 ms | 4.25 ms | 7.53 ms |

The demo's bar reads its animated width while it draws, so its frames
neither compose nor lay out; so does the drawn line. A composed line runs
composition and layout each frame, more code than fits: most of it stays
in the cache.

## Memory

- ESP32-S3: IRAM and the heap share the same internal RAM, so the section
  (125 KB of functions, 132 KB with their literals) is that much less internal heap:
  the demo has 97 KB of it free with the section and 226 KB without. With
  PSRAM, the heap takes larger blocks from PSRAM.
- Classic ESP32: the section takes 67 KB of IRAM, which the heap otherwise
  only lends to 32-bit allocations.

`CONFIG_CRANPOSE_HOT_CODE_IN_IRAM` (on by default, under "Cranpose" in
menuconfig) turns it off, for an application that needs the memory more.

## How the functions are chosen

`scripts/esp32/sampler` samples the PC 5000 times a second while the demo
runs, and `scripts/esp32/hot_code.py` picks the framework functions with the
most samples per byte until the budget is full. It takes several profiles,
each with the ELF it ran, and weighs them the same: a function counts with
its share of each profile's framework samples. The lists come from three
profiles of the demo: as it is, whose frames draw its bar again, and with a
line of text that changes every frame, drawn by a draw scope
(`--feature text-drawn`) or composed in a `Text` (`--feature text-composed`).
Functions of the profiled app, and generic functions instantiated with its
types, stay out: another app has other ones. The names in the scripts are
patterns of the v0 mangled names in which crate hashes and back-references
are wildcards (`scripts/esp32/v0_pattern.py`), so they match the same
functions in any build and any app. A pattern that matches nothing costs
nothing; a function renamed since the profile runs from the cache again.

A large function of which a frame runs a small part takes as much IRAM as
the whole of it. The app shell keeps the bodies a frame seldom runs, such
as moving focus into a modal or clearing pointer and focus flags, out of
line (`#[inline(never)]` after a cheap check), so its frame driver stays
small enough to share the budget.

To profile a board and write its script again, after changes that move the
framework's per-frame work:

```sh
for mode in demo drawn composed; do just esp32-profile es3c35p $mode; done
python3 scripts/esp32/hot_code.py 125 crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32s3.ld --board "apps/esp32-demo on the ES3C35P" \
  --profile target/esp32/profile-es3c35p-demo.elf target/esp32/profile-es3c35p-demo.log \
  --profile target/esp32/profile-es3c35p-drawn.elf target/esp32/profile-es3c35p-drawn.log \
  --profile target/esp32/profile-es3c35p-composed.elf target/esp32/profile-es3c35p-composed.log
```

The same for the E32R35T, with a budget of 64 KB, writes `esp32.ld`. The
budget counts the functions' code; their literals take a few KB more in the
section, 7 KB on the ES3C35P.

The profile runs without this component (`scripts/esp32/sdkconfig.no_hot_code`),
so its samples show what the cache costs, and prints the hottest functions
and crates.
