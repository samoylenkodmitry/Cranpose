# Attribution

`opus-pure` is derived from [`rusty-opus`](https://github.com/Remade-With-Rust/rusty-opus)
0.9.1 (BSD-3-Clause), itself a fork of [`opus-rs`](https://github.com/restsend/opus-rs)
(BSD-3-Clause).

Both are Rust ports of the reference [libopus](https://opus-codec.org/) implementation
by the Xiph.Org Foundation, Skype Limited, Octasic, Jean-Marc Valin, Timothy B. Terriberry,
CSIRO, Gregory Maxwell, Mark Borgerding and Erik de Castro Lopo. Module and function names
deliberately mirror the reference C sources so the two can be diffed against each other.

The original copyright notices and license terms are in [LICENSE](LICENSE) and apply to
this crate in full. The work described under *What differs* below is copyright Stephen
Berry, released under those same BSD-3-Clause terms, and its notice sits alongside the
inherited ones rather than replacing them.

## Code taken directly from libopus

The SILK downsampling filter coefficients in `src/silk/resampler.rs` — the 3:4, 2:3, 1:2,
1:3, 1:4 and 1:6 tables — are transcribed from `silk/resampler_rom.c` in libopus 1.6.1,
under the same BSD-3-Clause terms.

## What differs from the upstream fork

The Ogg container layer (`src/ogg/`) and the packet-inspection helpers (`src/packet.rs`)
are new. The rest is the upstream codec with the development scaffolding removed, the
public API narrowed, and a long list of defects fixed. The libopus comparisons behind
the larger ones are in [docs/interop-validation.md](docs/interop-validation.md).
