# basisu

A pure-Rust transcoder for [Basis Universal](https://github.com/BinomialLLC/basis_universal)
supercompressed GPU textures. It reads a `.ktx2` or `.basis` Basis texture and transcodes
it, at load time, into whatever block-compressed format the target GPU supports:
BC7 and the other BCn formats on desktop, ASTC and ETC on mobile, plus PVRTC,
ATC, FXT1, and uncompressed RGBA fallbacks.

Basis Universal addresses GPU texture-format fragmentation with a compress-once,
transcode-at-load model. You encode an image once into a compact intermediate,
ship that single file everywhere, and at load time transcode it (without
re-encoding) into the GPU-native format the device actually wants. This crate is
the transcoder half of that pipeline, the runtime decode side. The encoder is the
upstream `basisu` toolchain.

The implementation is a port of the reference C++ transcoder, verified
byte-for-byte identical to it across a corpus of real textures.

## Quick start

```toml
[dependencies]
basisu = "0.1"
```

The library name is `basisu`, so you write `use basisu::...`:

```rust
use basisu::{Transcoder, TargetFormat, DecodeFlags};

let bytes = std::fs::read("texture.ktx2")?;
let tex = Transcoder::new(&bytes)?;

// transcode mip level 0 to ASTC 4x4
let astc = tex.transcode(0, TargetFormat::Astc4x4Rgba, DecodeFlags::NONE)?;
```

Pick the target at runtime from what the GPU reports, so one shipped file
serves every device:

```rust
let target = if gpu.supports_astc() {
    TargetFormat::Astc4x4Rgba
} else if gpu.supports_bc7() {
    TargetFormat::Bc7Rgba
} else {
    TargetFormat::Rgba32 // uncompressed fallback
};

for level in 0..tex.level_count() {
    let data = tex.transcode(level, target, DecodeFlags::NONE)?;
    upload_mip(level, &data);
}
```

## How it works

A Basis texture stores its image data in a source codec chosen when the file
is encoded:

- **ETC1S** is a supercompressed subset of ETC1. It is small (roughly 0.3 to 3
  bits per pixel after the BasisLZ entropy stage) at low to medium quality, and
  transcodes very fast. A good default when size dominates.
- **UASTC LDR** is a high-quality 4x4 codec (8 bits per pixel) built on the ASTC
  bit layout. It is larger, but transcodes to high-quality targets like BC7 and
  ASTC with little quality loss. A good choice for normal maps and detail-heavy
  content.
- **UASTC HDR** is the high-dynamic-range variant: every block is a restricted
  ASTC HDR 4x4 block, transcoding to BC6H, ASTC HDR, or half-float/RGB-9E5
  pixels.

Transcoding turns that source codec into a specific GPU format without a full
decode and recompress. Each source block maps onto the target block format
directly, which is fast enough to run while a texture loads. The same file
becomes BC7 on a desktop, ASTC on a phone, and uncompressed RGBA where neither is
available, with no per-platform asset builds.

You produce `.ktx2` or `.basis` Basis files with the upstream encoder (`basisu`, or `toktx`
from KTX-Software). This crate consumes them.

## Supported formats

Every source codec the reference transcoder accepts is implemented:
**ETC1S**, **UASTC LDR**, **UASTC HDR 4x4**, **raw ASTC LDR** (all 14 block
sizes, 4x4 through 12x12), **raw ASTC HDR 6x6**, **UASTC HDR 6x6** (the
bitwise-compressed intermediate format), and **XUASTC LDR** (all 14 block
sizes; the arithmetic- or zstd-coded intermediate format). All decode
bit-exactly.

Every transcoder target for these codecs is implemented, 500 source/target
combinations verified byte-identical to the reference:

| Family | Targets |
|---|---|
| Desktop (S3TC, RGTC, BPTC) | BC1 (RGB), BC3 (RGBA), BC4 (R), BC5 (RG), BC7 (RGBA) |
| Mobile (ETC) | ETC1 (RGB), ETC2 (RGBA), EAC R11, EAC RG11 |
| ASTC | ASTC 4x4 (RGBA) |
| PowerVR | PVRTC1 4bpp (RGB, RGBA), PVRTC2 4bpp (RGB, RGBA) |
| Other | ATC (RGB, RGBA), FXT1 (RGB) |
| Uncompressed | RGBA32, RGB565, BGR565, RGBA4444 |
| HDR (from the HDR sources) | BC6H, ASTC 4x4/6x6 HDR, RGB/RGBA half-float, RGB 9E5 |

Not every target is valid for every source. UASTC LDR does not transcode to
PVRTC2, ATC, or FXT1; ETC1S covers all of them; raw ASTC LDR and XUASTC LDR
reach the 15 LDR targets plus only their own block size's ASTC pass-through.
The HDR sources transcode only to HDR targets (each reaching only its own
block size's ASTC pass-through), and the LDR sources never do.
`Transcoder::supports(target)` reports what a given texture can produce.

Container features: multi-mip textures, cubemaps, texture arrays,
Zstandard-supercompressed levels (with the optional `zstd` feature), and shared
(global) ETC1S codebooks across `.basis` files.

## API

`Transcoder::new(bytes)` parses and prepares a texture. The rest queries it and
transcodes:

- `transcode(level, target, flags) -> Vec<u8>` transcodes one mip level.
- `transcode_image(level, layer, face, target, flags)` selects a cubemap face or
  array layer.
- `transcode_into(level, target, flags, out)` writes into a caller-provided
  buffer without allocating the output (codecs that inherently need scratch,
  such as a Zstandard-compressed level or an intermediate stream, still
  allocate that internally, as the reference transcoder does).
- `transcode_video_frame(state, level, frame, target, flags)` decodes one
  ETC1S video frame; frames decode in order against a caller-owned
  `VideoState`.
- `etc1s_codebook()` lifts a self-contained ETC1S file's shared codebook out,
  and `Transcoder::new_with_codebook(bytes, &codebook)` opens a global-codebook
  `.basis` (one that references a codebook stored in a separate file) with it.
- `output_size(level, target)` returns the exact byte length a transcode
  produces.
- `image_level_info(level)`, `base_dimensions()`, `level_count()`,
  `layer_count()`, `face_count()` describe the contents.
- `source_format()`, `has_alpha()`, `supercompression()`, `is_video()`,
  `supports(target)` report capabilities.

`TargetFormat` and `DecodeFlags` carry the same numeric values as the C++
`transcoder_texture_format` and `basisd_decode_flags`. The flags cover the
reference decode options, such as `HIGH_QUALITY` for higher-quality UASTC to BCn
transcodes and `TRANSCODE_ALPHA_TO_OPAQUE`.

## Features

- `std` (default): the standard library. On std the crate has no required
  dependencies and no C or C++.
- `zstd` (default): decode Zstandard-supercompressed KTX2 levels via the
  pure-Rust `ruzstd`. Without it, such levels return `Error::ZstdRequired`
  (most ETC1S files use BasisLZ and need nothing here).

The crate also builds `#![no_std]` (with `alloc`) for embedded use: disable
`std` and enable the `libm` feature. That is a niche path most consumers can
ignore.

## Relationship to the upstream project

This is a Rust port of the Basis Universal C++ transcoder, tracking upstream
**v2_1_0**. It is not a binding: the published crate contains no C++. It is a
derivative work: the port was written against the reference source, and the
lookup tables are generated from the upstream table sources rather than
transcribed by hand.

## Status

- LDR transcoding is complete: all 21 target formats, both source codecs, every
  mip level and decode-flag variant, byte-identical to the reference.
- Container support: both `.ktx2` and `.basis`, cubemaps, texture arrays,
  Zstandard levels, and ETC1S video are done. Video frames decode in order
  through `transcode_video_frame` with a caller-owned `VideoState` (P-frames
  replenish from the previous frame); the stateless entry points detect video
  (`Transcoder::is_video`) and reject it rather than decode P-frames as
  garbage.
- Shared (global) ETC1S codebooks are done: a `.basis` file that references a
  codebook stored in a separate file decodes through
  `Transcoder::new_with_codebook` (the codebook is lifted from the donor file
  with `etc1s_codebook`), byte-identical to the reference's
  `set_global_codebooks`. KTX2 has no equivalent (its codebook is always
  embedded). The stateless entry points still reject a global-codebook file, so
  a caller that has no codebook never decodes garbage.
- UASTC HDR 4x4 is complete: BC6H, ASTC 4x4 HDR, half-float, and RGB-9e5
  output, byte-identical to the reference.
- Raw ASTC LDR sources (all 14 block sizes) are complete: the
  matching-block-size ASTC pass-through, all four uncompressed targets
  (with the deblocking filter), BC1/BC3/BC4/BC5, EAC R11/RG11, ETC1/ETC2,
  PVRTC1, and BC7. Every target is byte-identical to the reference over the
  full corpus.
- The 6x6 HDR sources are complete. Raw ASTC HDR 6x6: the 6x6 ASTC
  pass-through, BC6H (via the reference's real-time `fast_encode_bc6h`
  re-encoder, with the HIGH_QUALITY 2-subset search), half-float, and
  RGB-9E5 output. UASTC HDR 6x6 (the intermediate format, KTX2
  supercompression scheme 4 or raw `.basis` slices): the run/solid/reuse/
  block stream decompresses to standard ASTC 6x6 blocks, then reaches the
  same five targets.
- XUASTC LDR (all 14 block sizes) is complete: the three stream syntaxes
  (adaptive arithmetic, zstd side channels, and the hybrid) decompress to
  standard ASTC blocks, which reach the same-block-size ASTC pass-through
  and every LDR target, including the dedicated BC7 fast paths for the 4x4,
  8x6, and 6x6 sources and the fast-BC7-disable decode flag (the reference's
  `cDecodeFlagXUASTCLDRDisableFastBC7Transcoding`, value 1024).
- All 500 (source, target) combinations in the conformance matrix are verified
  byte-identical over the corpus.

One documented divergence exists on *malformed* input only: the pure-Rust
zstd decoder (`ruzstd`) rejects some corrupted zstd frames that libzstd
tolerates. Valid files are unaffected.

## Development

The repository is a workspace:

- `basisu` is the published crate (package name and library name are the same).
  Its tests and benches depend on repo-level files (`corpus/`, `goldens/`), so
  the published tarball excludes them; they run from the repository checkout.
- `conformance` is a dev-only crate. It compiles the vendored reference C++ as a
  golden oracle and runs the Rust transcoder against it, asserting zero differing
  bytes for every implemented format over the corpus. A committed
  `goldens/manifest.tsv` of SHA-256 hashes backs a pure-Rust replay test that
  needs no C++ toolchain.
- `xtask` drives the checks.

```sh
cargo xtask doctor        # what this machine can build/run, and how to fix the gaps
cargo xtask corpus        # fetch the full conformance corpus (~7800 files, ~220 MB)
cargo xtask verify        # conformance (byte-equality, needs a C++ compiler) and benchmarks
cargo xtask check         # fast pure-Rust path: fmt, clippy, golden replay
cargo xtask wasm          # wasm32 runtime tests under node
cargo xtask qemu          # bare-metal harnesses on emulated Cortex-M3 and M0
cargo xtask all           # check + verify + wasm + qemu (hard-fails on missing tooling)
cargo xtask ci            # the full local gate: all + MSRV + no_std + package dry-run
cargo xtask install-hooks # run `cargo xtask check` automatically on every git push
cargo xtask coverage      # transcoder-matrix coverage report
cargo xtask vendor <ref>  # re-vendor the reference C++ transcoder at an upstream git ref
cargo xtask bake-goldens  # re-bake goldens/manifest.tsv from the oracle (after vendoring)
```

Start with `cargo xtask doctor`: it reports every capability the gates use
(C++ compiler, system zstd, corpus, QEMU, wasm tooling) with the install
command for anything missing, on Linux and macOS alike. `cargo xtask all`
deliberately fails, rather than skips, when tooling is missing, so a green
`all` means the same thing on every machine.

The benchmarks in `verify` compare against a machine-local criterion baseline
named `pre-opt` when one exists (create it with `cargo bench -p basisu --bench
transcode -- --save-baseline pre-opt`); on a fresh checkout there is none, so
they run and record timings without a regression comparison.

### CI

This project uses no cloud CI: there are no workflow files, and nothing runs
on third-party infrastructure. `cargo xtask ci` runs every gate (check,
verify, wasm, qemu, MSRV, no_std, `cargo package` dry-run) on the machine it
is invoked on, keeps going after a failure so one report shows the full
picture, and writes a timestamped report to `target/ci/`. Run it after a
work session and before publishing. `cargo xtask install-hooks` wires the
fast `check` gate to git's pre-push hook, so every push is checked before it
leaves the machine (`git push --no-verify` bypasses it in an emergency).
Benchmarks are excluded from `ci` by design: timings are only meaningful
against the machine-local baseline described above.

The corpus lives under `corpus/`: a small committed `smoke/` set that every
test falls back to, plus the full set fetched by `cargo xtask corpus` (three
pinned, license-clean upstream sources). Running `cargo test -p conformance
--release` after that fetch reproduces the full byte-equality result (the
500/500 conformance matrix).

Two environment notes, both surfaced by `doctor`:

- The oracle decodes Zstandard-supercompressed files only when a system zstd
  is found at build time (Debian/Ubuntu `libzstd-dev`, Fedora `libzstd-devel`,
  macOS `brew install zstd`, or `ZSTD_INCLUDE_DIR`/`ZSTD_LIB_DIR`). Without
  it, the build prints a warning and the conformance run counts those files
  as `zstd-no-oracle` skips; the committed goldens still gate them in pure
  Rust.
- The wasm tests (`cargo xtask wasm`) need `cargo install wasm-bindgen-cli
  --version 0.2.118` (paired with the pinned `wasm-bindgen-test 0.3.68`) and
  node; the QEMU harnesses (`cargo xtask qemu`) need `qemu-system-arm` plus
  the two thumb targets (`rustup target add thumbv7m-none-eabi
  thumbv6m-none-eabi`).

### Staying in sync with upstream

The vendored C++ oracle is the reference every format is proven against, so
moving to a newer upstream release goes like this:

1. `cargo xtask vendor <ref>` re-vendors the transcoder-only source set at the
   given git ref, leaving a reviewable diff under
   `conformance/csrc/basisu_transcoder/`.
2. Pin the new ref in that directory's `UPSTREAM_VERSION`.
3. `cargo xtask verify` runs the Rust transcoder against the new oracle; any
   behavioral change surfaces as a failure localized to the exact (format,
   file) rows that diverged.
4. Port each divergence in the Rust code, repeating step 3 until it is green.
5. `cargo xtask bake-goldens` refreshes the committed `goldens/manifest.tsv`,
   so the toolchain-free `cargo xtask check` gate tracks the new reference.

## License

Apache-2.0. The vendored C++ under `conformance/csrc/basisu_transcoder/` is Basis
Universal (Apache-2.0, (c) Binomial LLC) and is used only as the test oracle. It
is not part of the published crate.
