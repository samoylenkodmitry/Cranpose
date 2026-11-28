#!/usr/bin/env bash
# Installs what ESP32-S3 builds need: the Xtensa Rust toolchain, ldproxy,
# espflash and Espressif's QEMU. ESP-IDF itself downloads into ~/.espressif on
# the first build.
set -euo pipefail

toolchain=esp-1.100.0
toolchain_version=1.100.0.0
qemu_tag=esp-develop-9.2.2-20260417
qemu_version=esp_develop_9.2.2_20260417
qemu_dir="$HOME/.local/esp-qemu"

cargo install --locked espup ldproxy espflash

if rustup toolchain list | grep -q "^$toolchain"; then
    echo "toolchain $toolchain is installed"
else
    espup install --name "$toolchain" --toolchain-version "$toolchain_version" \
        --targets esp32,esp32s3 --std --export-file "$HOME/export-$toolchain.sh"
fi

if [[ ${1:-} == --boards-only ]]; then exit 0; fi

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)
        host=aarch64-apple-darwin
        qemu_sha256=bb8c15810565d3df1665dc34962430885e11bc95575b228fb44698146be1e9d6
        ;;
    Darwin-x86_64)
        host=x86_64-apple-darwin
        qemu_sha256=ae8170fe46bcdfa54a7c0d7afcdb7a066711991be680f72ff7bbc6c4ae3ad88f
        ;;
    Linux-x86_64)
        host=x86_64-linux-gnu
        qemu_sha256=0eecb2a34a5586c0e59110f77b9343b7b336e82fdb0e1a30e1dc1bab8a547e35
        ;;
    Linux-aarch64)
        host=aarch64-linux-gnu
        qemu_sha256=00de5985094c14e47d1b38464a006b8ed64fd0fa7a289c56da24f3a329f65339
        ;;
    *) echo "no Espressif QEMU build for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

verify_sha256() {
    local expected="$1" file="$2"
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$expected  $file" | sha256sum --check --quiet
    else
        echo "$expected  $file" | shasum -a 256 --check --quiet
    fi
}

if [ -x "$qemu_dir/qemu/bin/qemu-system-xtensa" ]; then
    echo "QEMU is installed in $qemu_dir"
else
    archive="qemu-xtensa-softmmu-$qemu_version-$host.tar.xz"
    mkdir -p "$qemu_dir"
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --output "$qemu_dir/$archive" \
        "https://github.com/espressif/qemu/releases/download/$qemu_tag/$archive"
    verify_sha256 "$qemu_sha256" "$qemu_dir/$archive"
    tar -xf "$qemu_dir/$archive" -C "$qemu_dir"
fi

if [ "$(uname -s)" = Darwin ] && ! brew list --versions libgcrypt pixman sdl2 >/dev/null; then
    brew install libgcrypt pixman sdl2
fi
