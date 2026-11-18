#!/usr/bin/env bash
# Compiles the GLSL of the Android video import into the SPIR-V that the renderer embeds.
#
# usage: scripts/dev/compile_ycbcr_shaders.sh [--check]
#
# A YCbCr sampler needs a combined image sampler, which wgpu's shader languages cannot write, so
# the pass is GLSL. The script needs `glslc` and `spirv-val`: glslc comes with the Android NDK
# (`shader-tools`), and `spirv-val` with SPIRV-Tools. `--check` writes nothing and fails when a
# checked-in file differs from a fresh build.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
dir="$root/crates/cranpose/src/android"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"

glslc="$(command -v glslc || true)"
if [[ -z "$glslc" ]]; then
    for ndk in "${ANDROID_NDK_HOME:-}" "$sdk"/ndk/*; do
        for candidate in "$ndk"/shader-tools/*/glslc; do
            if [[ -x "$candidate" ]]; then
                glslc="$candidate"
            fi
        done
    done
fi
if [[ -z "$glslc" ]]; then
    echo "glslc is not installed: it ships in the Android NDK's shader-tools" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -r "$work"' EXIT

for source in ycbcr_cover.vert ycbcr_to_rgb.frag; do
    "$glslc" --target-env=vulkan1.1 -O "$dir/$source" -o "$work/$source.spv"
    spirv-val --target-env vulkan1.1 "$work/$source.spv"
    if [[ "${1:-}" == "--check" ]]; then
        cmp "$work/$source.spv" "$dir/$source.spv"
    else
        cp "$work/$source.spv" "$dir/$source.spv"
    fi
done
