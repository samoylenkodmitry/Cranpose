#!/usr/bin/env bash
# Runs the renderer's Vulkan synchronization tests under the Khronos
# validation layer, taken from Arch Linux's package and pinned by checksum.
set -euo pipefail

version=1.4.357.0-1
sha256=6f24c667b61e374b88789950f15e819cf4b32e9a1297a525520eed53d4077cf9
package="vulkan-validation-layers-$version-x86_64.pkg.tar.zst"
layer="${XDG_CACHE_HOME:-$HOME/.cache}/cranpose/vulkan-validation-layers-$version"

if [[ "$(uname -sm)" != "Linux x86_64" ]]; then
    echo "vulkan_sync.sh: needs Linux on x86_64 for the layer package" >&2
    exit 1
fi

if [[ ! -f "$layer/usr/lib/libVkLayer_khronos_validation.so" ]]; then
    download="$(mktemp -d)"
    trap 'rm -rf "$download"' EXIT
    curl -fsSL -o "$download/$package" \
        "https://archive.archlinux.org/packages/v/vulkan-validation-layers/$package"
    echo "$sha256  $download/$package" | sha256sum --check --quiet
    mkdir -p "$download/root"
    tar -C "$download/root" -xf "$download/$package" \
        usr/lib/libVkLayer_khronos_validation.so \
        usr/share/vulkan/explicit_layer.d/VkLayer_khronos_validation.json
    mkdir -p "$(dirname "$layer")"
    rm -rf "$layer"
    mv "$download/root" "$layer"
fi

VK_LAYER_PATH="$layer/usr/share/vulkan/explicit_layer.d" \
    LD_LIBRARY_PATH="$layer/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    VK_KHRONOS_VALIDATION_SYNCVAL_SHADER_ACCESSES_HEURISTIC=true \
    CRANPOSE_REQUIRE_SYNC_VALIDATION=1 \
    cargo nextest run --cargo-profile ci -p cranpose-render-wgpu --test integration vulkan_sync::
