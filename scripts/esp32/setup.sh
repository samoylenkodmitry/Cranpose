#!/usr/bin/env bash
# Installs what ESP32-S3 builds need: the Xtensa Rust toolchain, ldproxy,
# espflash and Espressif's QEMU. ESP-IDF itself downloads into ~/.espressif on
# the first build.
#
# The toolchain stays on esp 1.98.1: the std of esp 1.99.0.0 calls
# libc::fchmodat on ESP-IDF, which its libc does not define.
set -euo pipefail

toolchain=esp-1.98.1
toolchain_version=1.98.1.0
qemu_tag=esp-develop-9.2.2-20260417
qemu_version=esp_develop_9.2.2_20260417
qemu_dir="$HOME/.local/esp-qemu"

cargo install --locked espup ldproxy espflash

if rustup toolchain list | grep -q "^$toolchain"; then
    echo "toolchain $toolchain is installed"
else
    espup install --name "$toolchain" --toolchain-version "$toolchain_version" \
        --targets esp32s3 --std --export-file "$HOME/export-$toolchain.sh"
fi

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) host=aarch64-apple-darwin ;;
    Darwin-x86_64) host=x86_64-apple-darwin ;;
    Linux-x86_64) host=x86_64-linux-gnu ;;
    Linux-aarch64) host=aarch64-linux-gnu ;;
    *) echo "no Espressif QEMU build for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

if [ -x "$qemu_dir/qemu/bin/qemu-system-xtensa" ]; then
    echo "QEMU is installed in $qemu_dir"
else
    archive="qemu-xtensa-softmmu-$qemu_version-$host.tar.xz"
    mkdir -p "$qemu_dir"
    curl -fL --output "$qemu_dir/$archive" \
        "https://github.com/espressif/qemu/releases/download/$qemu_tag/$archive"
    tar -xf "$qemu_dir/$archive" -C "$qemu_dir"
fi

if [ "$(uname -s)" = Darwin ] && ! brew list --versions libgcrypt pixman sdl2 >/dev/null; then
    brew install libgcrypt pixman sdl2
fi
