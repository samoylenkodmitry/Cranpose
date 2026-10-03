#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
stable="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$SCRIPT_DIR/../../rust-toolchain.toml")"
nightly="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$SCRIPT_DIR/../../rust-toolchain-nightly.toml")"
"$SCRIPT_DIR/rustup_locked.sh" toolchain install --no-self-update "$stable" --profile minimal --component clippy --component rustfmt --component rust-src
"$SCRIPT_DIR/rustup_locked.sh" toolchain install --no-self-update "$nightly" --profile minimal --component rustfmt --component rust-src
