# Upstream forks

Crates Cranpose ships patched, under its own names, until the fix is in a
released upstream version. Each is the crates.io source of the upstream
version, renamed, with Cranpose's patch on top as its own commit. The forks
start at the upstream version, build as path dependencies outside the
workspace, and are published by `.github/workflows/publish.yml` ahead of the
Cranpose crates. Publishing skips a version crates.io already has, so a change
to a fork takes that crate's next patch version. Every fork above it that
names it directly then requires that version and takes its own next patch
version, up to the workspace's `wgpu` requirement, so a build that updates
Cranpose cannot keep the old crate in its lockfile. The repository's
formatting, spelling and diff gates skip the forks.

## wgpu 30.0.1

Upstream: <https://github.com/gfx-rs/wgpu>, commit
`40f4a34ebaf56f9a046231f54125ad046239d3f3` (`wgpu-hal` 30.0.1).
`cranpose-wgpu-hal` is at 30.0.3 for the catch-up barrier and the Metal
pipeline switch below, and `cranpose-wgpu-core` and `cranpose-wgpu` are at
30.0.3 to require it.

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
stage other than fragment or compute read a texture. Until one does, a
sampled texture's barrier waits only in the fragment and compute stages;
command buffers begun after one does wait in upstream's stages: vertex,
fragment and compute. Upstream names no task or mesh stage in any barrier,
so experimental mesh shading stays as unsynchronized as it is upstream.
Cranpose's renderer samples textures in fragment shaders only.

A texture an earlier buffer left in a read state takes no new barrier, so
a vertex shader in a later buffer could read it before the earlier
buffer's writes are visible to it. Where a submission moves from buffers
begun before the change to buffers begun after it, the queue runs one
recorded barrier that orders all earlier work and writes before all later
work. Separate submissions already run one after another, and waits for
earlier reads need nothing more: a barrier's source stages include every
logically earlier stage. `just vulkan-sync` checks these cases under the
Khronos layer's synchronization validation.

On a Huawei Mate 20 X (Mali-G76), Showcase scrolling went from 29.9 ms to
21.7 ms a frame (4 ABBA legs each). Dawn ships the per-binding version of
the same fix ("Improve Vulkan synchronization").

### Compact Vulkan diagnostics

The fork also defines compact `Debug` output for Vulkan samplers and texture
views. These implementations omit the Vulkan create-info and raw format fields.
The smaller output lets the linker remove ash's large enum-name formatters.
Commit `aea67c5d8` records an ARM64 benchmark-library reduction of 85,872 bytes.

### Metal pipeline switches copy only vertex buffer locations

A Metal render pass copies the bound pipeline's stage info into its state at
every `set_render_pipeline`. Upstream keeps the stage's naga vertex buffer
mappings there, each owning a vector of attributes, so every switch
allocated once per mapping: about 2,100 allocations a frame in Cranpose's
gauntlet benchmark at tier 12 on an M5. The pass reads only each mapping's
shader location, to put that buffer's size in the sizes buffer. The fork
keeps those locations (`vertex_buffer_ids`), which a switch copies into the
capacity it already holds. Shader compilation still receives the full
mappings.

### Update the fork

1. Copy the new upstream versions' crates.io sources over the crate
   directories (`src`, `build.rs`, `Cargo.toml`, licences, readme), as one
   commit.
2. Rename the packages and point the forked dependencies at their sibling
   directories (`package = "cranpose-..."`, `path = "../..."`), as before.
3. Reapply the patch, or drop the fork once upstream ships the fix.
