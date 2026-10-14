#!/usr/bin/env bash
# The minimum Rust version the crates declare is the oldest toolchain CI builds
# them with: the ESP32 firmware's Xtensa toolchain, which follows the
# rust-toolchain.toml pin with a delay. `just msrv-check` builds the workspace
# with that version every night. A toolchain bump without rust-version, or the
# other way round, would promise a toolchain nobody tested.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
minor() { sed -n 's/^\([0-9]*\.[0-9]*\).*/\1/p' <<< "$1"; }
pinned="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")"
esp="$(sed -n 's/^channel = "esp-\(.*\)"/\1/p' "$root/apps/esp32-demo/rust-toolchain.toml")"
recipe="$(sed -n 's/^esp32_toolchain := "esp-\(.*\)"/\1/p' "$root/justfile")"
declared="$(sed -n '/^\[workspace.package\]/,/^\[/s/^rust-version = "\(.*\)"/\1/p' "$root/Cargo.toml")"
if [ "$esp" != "$recipe" ]; then
    echo "FAIL: apps/esp32-demo pins esp-${esp} but the justfile's esp32_toolchain is esp-${recipe}" >&2
    exit 1
fi
if [ "$(minor "$esp")" != "$declared" ]; then
    echo "FAIL: the oldest CI toolchain is esp-${esp} but [workspace.package] rust-version is '${declared}'" >&2
    exit 1
fi
if [ "$(printf '%s\n%s\n' "$esp" "$pinned" | sort -V | head -1)" != "$esp" ]; then
    echo "FAIL: the ESP32 toolchain esp-${esp} is newer than the pinned ${pinned}" >&2
    exit 1
fi
missing=""
while IFS= read -r manifest; do
    if grep -q '^version.workspace = true\|^version = { workspace = true }' "$manifest" \
        && ! grep -q '^rust-version.workspace = true\|^rust-version = { workspace = true }' "$manifest"; then
        missing="$missing $manifest"
    fi
done < <(find "$root/crates" -name Cargo.toml -not -path '*/target/*')
if [ -n "$missing" ]; then
    echo "FAIL: these crates take the workspace version but not its rust-version:$missing" >&2
    exit 1
fi
echo "ok: crates declare rust-version ${declared}, the ESP32 toolchain esp-${esp}; the pin is ${pinned}"
