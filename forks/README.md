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
`cranpose-wgpu-hal` is at 30.0.8 for the catch-up barrier, the Metal
pipeline switch, the dedicated Vulkan allocations, the GL point size, the
GL state tracker, the web surface usages, the boxed GL copy commands and
the Vulkan command pools that give a burst's memory back below, and
`cranpose-wgpu-core` and `cranpose-wgpu` are at 30.0.8 to require it.

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
`wgpu_hal`, ...), so code reads the same. `wgpu-hal` and the WebGPU backend
of `wgpu` change; `wgpu-core` is forked because each crate depends on the
one below it by name.
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

### Large Vulkan resources take dedicated allocations

`gpu-allocator` gives a resource memory of its own only when the resource
is larger than a memory block; a smaller one takes a range of a shared
block. With Cranpose's mobile memory hints a block of host-visible memory
is 4 MiB, and every memory type of a Mali GPU is host-visible. Small
textures and tables of other lifetimes stay scattered through every
shared block, so its free space splits into small ranges. On a Huawei
Mate 20 X (Mali-G76) at gauntlet tier 12, a block with 2.9 MiB free held no
free 512 KiB range, and each frame ring buffer of 0.8 to 1.4 MiB and each
512 KiB glyph chunk the renderer opened after the first frames took a new
4 MiB block. The GL memory Android reports for the process stays at the
allocator's peak.

The fork's Vulkan device gives a buffer or texture whose memory takes at
least an eighth of the smallest configured block a dedicated allocation
(`VkMemoryDedicatedAllocateInfo`, core since Vulkan 1.1; a Vulkan 1.0
device keeps upstream's behaviour). With the mobile hints that is 512 KiB
and up; with wgpu's default `Performance` hints it is 8 MiB and up. On the
Mate the allocator's peak over three launches was 40.3 to 41.6 MiB, against
42.4 to 46.4 MiB before.

### GL programs write the point size only to draw points

Upstream's GL backend makes every vertex shader write `gl_PointSize`, so a
pipeline that draws points gets a defined size. WebGL counts that output
against the 15 varying vectors a program may link with (GLSL ES 1.00,
appendix A.7), and so does it count `gl_FragCoord`, which a fragment stage
reading `@builtin(position)` uses. On a Huawei Mate 20 X (Mali-G76, Chrome
154, ANGLE on OpenGL ES) a program with 14 vector varyings, the position
and the point size failed to link ("Could not pack varying gl_PointSize"),
and the same program without the point size linked. The general shape
pipeline of Cranpose's renderer has exactly those 14 vectors, so every
frame that drew with it failed validation and drew nothing.

The fork's GL device writes `gl_PointSize` only in the vertex stage of a
pipeline whose topology is a point list; the program cache keys on it.

### The GL queue skips calls that set what the context holds

Upstream's GL encoder re-specifies every vertex attribute of a pipeline at
each draw whose buffers or first instance moved (four calls an attribute),
disables them all at each pipeline switch, and binds the scissor, textures,
samplers and uniform ranges again for every batch. On WebGL each call
crosses into JavaScript, is validated and serialized by the renderer, and
is decoded and validated again in the GPU process.

The fork's queue keeps the state a command buffer's commands have set
(`gles/gl_state.rs`): attribute enables, pointers and divisors, the
`ARRAY_BUFFER` binding, the program, scissor, depth and stencil state,
uniform buffer ranges, textures, samplers and the active unit. A command
that sets a value the context holds makes no call; disables wait for the
next draw, since the next pipeline usually enables the same attributes.
Any command the tracker does not know forgets it all, and the state starts
unknown in each command buffer, as the device's own calls run between
them. A draw with no first-instance uniform makes no `uniform1ui(null)`.

Cranpose's gauntlet at tier 8 made 24,200 WebGL calls a frame, 74% of them
attribute calls, and makes 9,800 with the same pictures. On a Mate 20 X in
Chrome 154 (6 legs each) the renderer's main thread went from 16.6 to
16.1 ms a frame and the GPU process's from 7.6 to 7.0 ms.

### Web surfaces can be copied and sampled; the WebGL canvas has no depth

Upstream reports only `RENDER_ATTACHMENT` for a WebGPU canvas, which takes
copies and sampling as well, and only `COLOR_TARGET` for a WebGL surface,
whose image is a texture of the GL backend's own. Cranpose draws a frame
straight into the presented image only when it can copy it and, for a
backdrop, sample it; otherwise it draws into a texture of the viewport's
size and converts that into the image in one more full-screen pass. The
fork reports the copies and sampling for both.

A WebGL canvas also took the default depth buffer (8.1 MiB at
1080 x 1975), which no draw uses: the GL backend renders into its surface
texture and blits that to the canvas. The fork asks for a canvas without
depth or stencil.

### GL copy and attachment commands are boxed

The GL encoder records each command into a vector that the queue plays
back at submission, and every command took the size of the largest: 128
bytes on wasm32, set by the buffer and texture copies. The commands of a
frame stay alive until the GPU finishes it, several frames at once. The
fork boxes the payloads of the rare large commands (buffer clears, copies,
query result copies, attachment binds and resolves), so a command takes 48
bytes, the size of a vertex attribute.

Cranpose's gauntlet at tier 8 records about 6,100 commands for its main
pass, 59% of them vertex attribute commands: the pass's vector held 1 MiB
and holds 384 KiB. In Chrome on an M5 at the Mate 20 X's viewport, on
WebGL, the page's peak heap went from 22.2 to 20.7 MiB and its WebAssembly
memory from 28.1 to 25.8 MiB.

### Vulkan command pools give a burst's memory back

`wgpu-core` keeps every command encoder it made and reuses it once its
submission is done; `wgpu-hal` resets the encoder's Vulkan command pool
without `RELEASE_RESOURCES`, so the pool keeps the memory its command
buffers took. A tile-based driver takes command memory for each render
pass. On a Pixel 9 Pro (Mali-G715) the first frames of Cranslate's main
screen record 17 to 48 passes each while its glass panels capture and blur
what lies behind them, about 1,000 passes in 350 ms, and the driver took
that memory in chunks of 128 KiB and 576 KiB. The pools kept it: the GL
memory Android reported was 255 to 265 MiB with the screen idle after
launch, and 314 to 321 MiB after scrolling its lines, against 37 MiB for
a bench app without glass.

The fork's Vulkan encoder counts the render passes begun since its pool's
last reset (`vulkan/pool_trim.rs`). A pool that held at least 8 passes
gives its memory back, with `RELEASE_RESOURCES`, at the fourth reset in a
row that used less than half of that, and counts from that reset again.
A steady workload, light or heavy, keeps its memory, so its frames record
as fast as before; heavy frames between light ones keep it too.

On the Pixel, the idle main screen after launch held 161 to 177 MiB, and
after scrolling 161 to 181 MiB. Over the same scroll the present thread's
median render took 2.88 and 2.98 ms a frame against 3.13 and 3.28 ms
before (two legs each). Giving the memory back at every reset reached
142 MiB but cost the present thread 0.2 to 0.3 ms a frame.

### Update the fork

1. Copy the new upstream versions' crates.io sources over the crate
   directories (`src`, `build.rs`, `Cargo.toml`, licences, readme), as one
   commit.
2. Rename the packages and point the forked dependencies at their sibling
   directories (`package = "cranpose-..."`, `path = "../..."`), as before.
3. Reapply the patch, or drop the fork once upstream ships the fix.
