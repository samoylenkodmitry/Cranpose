#!/usr/bin/env bash
set -euo pipefail

# The linker driver cargo invokes for the robot examples, as a wrapper rather
# than a rustflag: `CARGO_TARGET_<triple>_RUSTFLAGS` REPLACES the `[build]
# rustflags` in .cargo/config.toml rather than adding to them, which would
# silently drop the --remap-path-prefix pair every panic message depends on.
# Setting only the linker leaves that list alone.
exec cc -fuse-ld=mold "$@"
