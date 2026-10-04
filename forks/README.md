# Upstream forks

Crates Cranpose ships patched, under its own names, until the fix is in a
released upstream version. Each is the crates.io source of the upstream
version, renamed, with Cranpose's patch on top as its own commit. The forks
keep the upstream version, build as path dependencies outside the workspace,
and are published by `.github/workflows/publish.yml` ahead of the Cranpose
crates. The repository's formatting, spelling and diff gates skip them.

## wgpu 30.0.1

Upstream: <https://github.com/gfx-rs/wgpu>, commit
`40f4a34ebaf56f9a046231f54125ad046239d3f3` (`wgpu-hal` 30.0.1).

| fork | upstream |
| --- | --- |
| `cranpose-wgpu` | `wgpu` |
| `cranpose-wgpu-core` | `wgpu-core` |
| `cranpose-wgpu-hal` | `wgpu-hal` |
| `cranpose-wgpu-core-deps-apple` | `wgpu-core-deps-apple` |
| `cranpose-wgpu-core-deps-emscripten` | `wgpu-core-deps-emscripten` |
| `cranpose-wgpu-core-deps-wasm` | `wgpu-core-deps-wasm` |
| `cranpose-wgpu-core-deps-windows-linux-android` | `wgpu-core-deps-windows-linux-android` |

Every library keeps its upstream crate name (`wgpu`, `wgpu_core`,
`wgpu_hal`, ...), so code reads the same. Only `wgpu-hal` changes; the other
crates are forked because each depends on the one below it by name.
`wgpu-types`, `naga` and `wgpu-naga-bridge` stay upstream.

### The patch: sampled textures wait only in the stages that read them

`wgpu-hal`'s Vulkan backend gives a texture's sampled use (`TextureUses::RESOURCE`)
the barrier stages vertex, fragment and compute. A pass that samples what
the previous pass drew therefore holds its vertex shading until that pass's
fragment work is done. Tile-based GPUs run vertex and fragment work in
separate queues, so on a Mali this drains the GPU at every dependent pass.

The fork's Vulkan device records whether any bind group layout lets a
vertex shader read a texture. Until one does, a sampled texture's barrier
waits only in the fragment and compute stages; once one does, the device
keeps upstream's stages. Cranpose's renderer samples textures in fragment
shaders only.

On a Huawei Mate 20 X (Mali-G76), Showcase scrolling went from 29.9 ms to
21.7 ms a frame (4 ABBA legs each). Dawn ships the per-binding version of
the same fix ("Improve Vulkan synchronization").

### Updating

1. Copy the new upstream versions' crates.io sources over the crate
   directories (`src`, `build.rs`, `Cargo.toml`, licences, readme), as one
   commit.
2. Rename the packages and point the forked dependencies at their sibling
   directories (`package = "cranpose-..."`, `path = "../..."`), as before.
3. Reapply the patch, or drop the fork once upstream ships the fix.
