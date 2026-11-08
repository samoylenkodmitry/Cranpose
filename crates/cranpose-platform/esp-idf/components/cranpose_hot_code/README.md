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
| ES3C35P (ESP32-S3), all code in flash | 63K | 699K | 81K |
| ES3C35P, this component on | 63K | 27K | 11K |
| E32R35T (classic ESP32), all code in flash | 57K | 754K | 104K |
| E32R35T, this component on | 57K | 264K | 48K |

This component links the framework functions that a frame runs most into
IRAM, the internal RAM that the CPU fetches without the cache. A linker
script for each chip lists them, `esp32s3.ld` and `esp32.ld`, and inserts an
output section after ESP-IDF's own IRAM code.

| Frame | demo | a drawn line of text changing every frame | a composed one |
| --- | --- | --- | --- |
| ES3C35P, without (2026-10-25) | 3.11 ms | 5.71 ms | 8.24 ms |
| ES3C35P, with 125 KB (2026-10-26) | 0.49 ms | 2.29 ms | 4.66 ms |
| E32R35T, without (2026-10-25) | 3.39 ms | 7.01 ms | 9.61 ms |
| E32R35T, with 64 KB (2026-10-26) | 1.33 ms | 4.00 ms | 6.89 ms |

The demo's bar reads its animated width while it draws, so its frames
neither compose nor lay out; so does the drawn line. A composed line runs
composition and layout each frame, more code than fits: most of it stays
in the cache.

## Memory

- ESP32-S3: IRAM and the heap share the same internal RAM, so the section
  (125 KB of functions, 132 KB with their literals) is that much less internal heap:
  the demo has 102 KB of it free with the section and 230 KB without. With
  PSRAM, the heap takes larger blocks from PSRAM.
- Classic ESP32: the section takes 68 KB of IRAM, which the heap otherwise
  only lends to 32-bit allocations.

`CONFIG_CRANPOSE_HOT_CODE_IN_IRAM` (on by default, under "Cranpose" in
menuconfig) turns it off, for an application that needs the memory more.

## How the functions are chosen

`scripts/esp32/hot_code.py` picks framework functions by their share of a
profile per byte until the budget is full. It takes several profiles, each
with the ELF it ran, and weighs them the same: a function counts with its
share of each profile's framework samples. Functions of the profiled app,
and generic functions instantiated with its types, stay out: another app has
other ones. The names in the scripts are patterns of the v0 mangled names in
which crate hashes and back-references are wildcards
(`scripts/esp32/v0_pattern.py`), so they match the same functions in any
build and any app. A pattern that matches nothing costs nothing; a function
renamed since the profile runs from the cache again.

The renderer draws 16-bit pixels high byte first for every panel, as the
boards' panels read them. QEMU's panel reads the low byte first, so its draw
swaps the bytes of each update. QEMU and the boards run the same drawing
functions, and an image links one copy of them.

The profiles come from the demo: as it is, whose frames draw its bar again;
with a line of text that changes every frame, drawn by a draw scope
(`text-drawn`) or composed in a `Text` (`text-composed`); and on the
ES3C35P with a busier screen (`workload`): readings at their own rates, a
panel that comes and goes, a counting button and a list that scrolls by
itself. The busier screen keeps the list from fitting the benchmark lines
alone: against a list of the three benchmark screens it runs the ES3C35P's
demo frame 0.07 ms and its drawn line 0.19 ms slower, and its composed line
0.12 ms faster. The E32R35T's 64 KB have no room for it: with its profile
the busier screen ran no faster there (12.31 ms a frame against 12.23) and
the demo frame 0.23 ms slower.

The ES3C35P's list also takes the dashboard of the benchmarks
(`--features dashboard`) as a fifth profile. Against the list of the other
four, made from the same builds on 2026-10-31, it ran the dashboard 25.39
against 25.61 ms a frame, the demo 0.41 against 0.45, the composed line
4.04 against 4.11 and the busier screen 7.94 against 8.02, and the drawn
line 1.65 against 1.58 ms.

Both lists were made again on 2026-10-31 after the pixel scene began to
write a changed subtree over its old items, whose new functions had run
from flash. Against main with the lists before, the ES3C35P ran the drawn
line 1.65 against 1.69 ms a frame, the composed line 4.02 against 4.07, the
dashboard 25.06 against 25.24 and the busier screen 7.90 against 8.00; the
E32R35T ran the demo 1.02 against 1.21 ms, the drawn line 3.51 against
3.83, the composed line 6.15 against 6.40 and the busier screen 9.58
against 9.62.

A list of 125 KB takes about 2 ms longer to load at reset than one of 89 KB
(the bootloader copies it from flash), and the screens whose first frame
waits for the panel show it then. An 89 KB list made from the same
profiles ran the dashboard slower than a list that had kept 89 KB of an
older build's functions (26.97 against 26.75 ms).

- `esp32s3.ld` comes from QEMU: `just esp32-sim-profile MODE` builds the
  demo for QEMU's ESP32-S3 without this component and counts the misses of
  the chip's 32 KB instruction cache with a QEMU plugin
  (`scripts/esp32/cache/`), and `scripts/esp32/sim_profile.py` writes them
  as a profile. A list by misses per byte holds the code that waits for flash
  the most; a list by sampled time also holds loops that the cache serves.
  On the same three screens the list by misses ran the ES3C35P's drawn line
  0.17 ms and its composed line 0.12 ms faster. The simulator's misses match
  the board's waits: 305 to 320 cycles each. With `--cache count` the plugin
  counts the instructions each function runs instead, and `--icount 2` runs
  the chip's clock on them: two runs of the dashboard's build then differ by
  141 of 1.92 million instructions a frame, where without it they differ by
  80 thousand. Two builds still draw other frames once their instruction
  counts differ, as the dashboard's animations follow that clock.
- `esp32.ld` comes from the board: `scripts/esp32/sampler` samples the PC
  5000 times a second. The simulator does not know the classic ESP32's
  cache yet, and its list ran the E32R35T's drawn line 0.24 ms slower.

## The rest of the sampled code in flash

A function that the profiles sampled but the budget left out runs from
flash. The linker would place it where its crate's code falls, among code a
frame never runs, and a change anywhere before it would move it and the
functions it shares the cache with: a 228-byte shift of the flash code moved
every E32R35T screen by 0.02 to 0.05 ms. A file for each chip,
`esp32s3.order` and `esp32.order`, lists the sampled functions hottest per
byte first, and the linker puts them at the start of the flash code
(`--section-ordering-file`), one after another, where they evict each other
less and code outside the list does not move them. A function of the IRAM
list matches the IRAM section first and stays in IRAM.

Both files come from the boards' sampled profiles, as `esp32.ld` does: an
order by QEMU's cache misses read the ES3C35P's dashboard 24.97 -> 25.01 ms
in a first trial, between two worktrees. Each build below comes from one
worktree with the order file removed or put back, as two worktrees' paths
give their crates other hashes and rustc other code (2026-11-01, main
ff6f10cf0):

| ms a frame | demo | drawn line | composed line | busier screen | dashboard |
| --- | --- | --- | --- | --- | --- |
| ES3C35P | 0.46 -> 0.42 | 1.68 -> 1.64 | 4.07 -> 4.03 | 7.84 -> 7.76 | 24.66 -> 24.58 |
| E32R35T | 0.99 -> 0.92 | 3.51 -> 3.46 | 6.21 -> 6.03 | 9.61 -> 9.54 | |

To write the scripts again, after changes that move the framework's
per-frame work:

```sh
scripts/esp32/cache/setup.sh    # once: QEMU with plugins, and the cache plugin
just esp32-sim-profiles   # the five screens, their QEMUs at once
python3 scripts/esp32/hot_code.py 124.6 crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32s3.ld \
  --board "apps/esp32-demo in QEMU's ESP32-S3, by cache misses" \
  --profile target/esp32/sim-demo.elf target/esp32/sim-demo.log \
  --profile target/esp32/sim-drawn.elf target/esp32/sim-drawn.log \
  --profile target/esp32/sim-composed.elf target/esp32/sim-composed.log \
  --profile target/esp32/sim-workload.elf target/esp32/sim-workload.log \
  --profile target/esp32/sim-dashboard.elf target/esp32/sim-dashboard.log
for mode in demo drawn composed workload dashboard; do just esp32-profile es3c35p $mode 30; done
python3 scripts/esp32/hot_code.py 0 /dev/null \
  --flash-order crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32s3.order \
  --board "apps/esp32-demo on the ES3C35P" \
  --profile target/esp32/profile-es3c35p-demo.elf target/esp32/profile-es3c35p-demo.log \
  --profile target/esp32/profile-es3c35p-drawn.elf target/esp32/profile-es3c35p-drawn.log \
  --profile target/esp32/profile-es3c35p-composed.elf target/esp32/profile-es3c35p-composed.log \
  --profile target/esp32/profile-es3c35p-workload.elf target/esp32/profile-es3c35p-workload.log \
  --profile target/esp32/profile-es3c35p-dashboard.elf target/esp32/profile-es3c35p-dashboard.log

for mode in demo drawn composed workload; do just esp32-profile e32r35t $mode; done
python3 scripts/esp32/hot_code.py 64 crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32.ld \
  --board "apps/esp32-demo on the E32R35T" \
  --profile target/esp32/profile-e32r35t-demo.elf target/esp32/profile-e32r35t-demo.log \
  --profile target/esp32/profile-e32r35t-drawn.elf target/esp32/profile-e32r35t-drawn.log \
  --profile target/esp32/profile-e32r35t-composed.elf target/esp32/profile-e32r35t-composed.log
python3 scripts/esp32/hot_code.py 0 /dev/null \
  --flash-order crates/cranpose-platform/esp-idf/components/cranpose_hot_code/esp32.order \
  --board "apps/esp32-demo on the E32R35T" \
  --profile target/esp32/profile-e32r35t-demo.elf target/esp32/profile-e32r35t-demo.log \
  --profile target/esp32/profile-e32r35t-drawn.elf target/esp32/profile-e32r35t-drawn.log \
  --profile target/esp32/profile-e32r35t-composed.elf target/esp32/profile-e32r35t-composed.log \
  --profile target/esp32/profile-e32r35t-workload.elf target/esp32/profile-e32r35t-workload.log
```

The budgets count the functions' code without their literals. On the
ES3C35P a list whose literals take more room than the last list's takes
internal heap, so its budget shrinks until the section is no larger than it
was: 124.6 KB. The classic ESP32 lends the IRAM the section leaves only to
32-bit allocations, which Cranpose never makes, so its budget stays 64 KB.

A budget of 0 leaves every sampled function to the order file, the ones of
the IRAM list too, which stay in IRAM. The E32R35T's busier screen joins the
order's profiles, which its IRAM list has no room for.

The budget counts the functions' code; their literals take a few KB more in
the section, 7 KB on the ES3C35P. The board profiles run without this
component (`scripts/esp32/sdkconfig.no_hot_code`), so their samples show what
the cache costs, and print the hottest functions and crates.

A large function of which a frame runs a small part takes as much IRAM as
the whole of it. Bodies that a frame seldom runs stay out of line after a
cheap check (`#[inline(never)]`): the app shell's pointer claims, mouse
moves, telemetry and post-layout recomposition, the pixel renderer's images,
shadows and covered shapes, and the platform's touch routing. The platform's
frame loop takes the touch panel as a trait object, so it is one function of
the platform crate in every app, which the lists can name, where it was
compiled into each app's `main`.
