# Upstream forks

Crates Cranpose ships patched, under its own names, until the fix is in a
released upstream version. Each is the crates.io source of the upstream
version, renamed, with Cranpose's patch on top as its own commit. The forks
start at the upstream version and are published by
`.github/workflows/publish.yml` ahead of the Cranpose crates. Cranpose's
crates name them by version; the workspace's `[patch.crates-io]` builds them
from this directory. Publishing skips a version crates.io already has, so a
change to a fork takes that crate's next patch version. Every fork above it
that names it directly then requires that version and takes its own next
patch version, up to the workspace's `wgpu` requirement, so a build that
updates Cranpose cannot keep the old crate in its lockfile. The repository's
formatting, spelling and diff gates skip the forks.

Every target builds with the wgpu fork: one wgpu, with the same fixes, on
Android, iOS, watchOS, macOS, Windows, Linux and the web. A game engine that
shares Cranpose's device, such as Bevy, takes the fork too through a
`[patch.crates-io]` of `wgpu` in the app's workspace. A patch stays only
while it gains enough to justify the fork; each one is offered upstream.

## wgpu 30.0.1

Upstream: <https://github.com/gfx-rs/wgpu>, commit
`40f4a34ebaf56f9a046231f54125ad046239d3f3` (`wgpu-hal` 30.0.1).
`cranpose-wgpu-hal` is at 30.0.17 for the catch-up barrier, the Metal
pipeline switch, the dedicated Vulkan allocations, the GL point size, the
GL state tracker, the web surface usages, the boxed GL copy commands, the
Vulkan command pools that give a burst's memory back, the GL shaders
without the branches their constants decide, the GL program status read
only on failure, the GLSL templates, the GL link reads, the WebGL
extensions, the GL varying minimum and the Vulkan swapchain image indices
below. `cranpose-wgpu-core` is at 30.0.19 to require those, to make
template modules, to configure a surface while another thread submits, to
let Vulkan pipelines drop their shader modules and to hand presented
swapchain images back as they presented. `cranpose-wgpu` is at 30.0.22 for
the WebGPU pipeline discovery, the background compiles, the pipeline
readiness and the kept swapchain images below and to require them.

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

### GL shaders drop the branches their constants decide

Upstream's GL backend applies a pipeline's override constants and writes
GLSL for the whole entry point. A condition such as
`DRAWS_ARCS & (kind == ARC)` keeps its runtime side after the constant
becomes `false`, so both sides of every such branch reach the driver,
which compiles and links all of it. In Chrome on a Huawei Mate 20 X
(Mali-G76, ANGLE on OpenGL ES) each of Cranpose's seven shape programs
at start took 400 to 600 ms to link, on the page's only thread, and the
page drew nothing for 5.9 s after its first frame.

Before it writes GLSL, the fork's GL device folds each condition as far as
its decided operands allow (`false & x`, `true | x`, comparisons and bit
operations of constants, a call to a function that only returns a decided
value), puts the side of each decided `if` in its place, compacts away the
functions only the dropped sides called and validates the module again
(`wgpu_hal::auxil::prune`). A solid fill program went from 31 KB of vertex
and 25 KB of fragment GLSL to 13 and 12 KB, and its link from 510 to 30
ms. The frozen gauntlet frame on WebGL is the same to the pixel.

### WebGL pipelines build from GLSL templates

Upstream's GL backend gets a pipeline's shaders as a naga module, which on
the web means parsing the WGSL, validating it and writing GLSL on the
page's only thread. On a first visit to the gauntlet in Chrome on a Huawei
Mate 20 X that was about 0.3 s before anything showed, besides compiling
naga's own WebAssembly.

`wgpu_hal::GlslTemplate` is GLSL ES written at build time for one entry
point, whose pipeline constants are preprocessor macros, with the names the
backend binds its blocks, textures and varyings by. `ShaderInput::GlslTemplates`
gives the GL device a module of them. For a pipeline, the device works out
the template's `#if`s from the pipeline's constants and hands the browser
only the code they keep, with the constants the code reads defined after
the `#version` line: no naga runs, and the browser's compiler reads no more
than it would of naga's pruned GLSL. `wgpu-core` makes such a module with
`Device::create_shader_module_glsl_templates`, and `wgpu` with the safe
`Device::create_shader_module_glsl_templates` on a WebGL device, where the
browser checks the GLSL. Cranpose's build writes the templates
(`cranpose-gl-templates`). On that first visit, wiped Chrome on the Mate,
the gauntlet's content came 1.95 and 2.09 s after navigation with naga,
1.74 and 1.73 s with the templates, and its frames are the same to the
pixel.

### GL programs read their shader status only when they fail to link

Upstream's GL device reads each shader's compile status and info log, then
the program's link status and info log, for every pipeline. In WebGL each
of these reads waits for a round trip to the GPU process, and the first
one also waits for the compile. The fork reads only the link status; a
failed link then reads the shaders' status and logs to report which stage
failed, and the program's log is read only for debug logging. In Chrome on
a Huawei Mate 20 X the gauntlet's pipeline creation at start went from
about 396 to 302 ms (two profiles each).

### WebGL enables only the extensions the device uses

`glow` asks for every WebGL extension it knows as it wraps a context, and
WebGL enables an extension when the page asks for it: each ask can wait
for the browser's GPU process. Chrome on a Huawei Mate 20 X offers 15 of
them, and asking for all took 16-24 ms of every start, before the page
could build a program. After `wgpu_hal::gles::ask_only_for_used_extensions`,
which Cranpose's web launcher calls, the device gives the context its own
`getSupportedExtensions` and `getExtension` while `glow` wraps it. They
answer only for the extensions `glow` uses or that name the adapter
(`WEBGL_debug_renderer_info`, `WEBGL_lose_context`,
`KHR_parallel_shader_compile`, `EXT_disjoint_timer_query_webgl2`), and the
device deletes them after, so the prototype's methods answer again. Such an
adapter reports capabilities for those extensions alone: float render
targets, anisotropic filtering, per-target blending, sample shading,
compressed and 16-bit normalized formats and multiview are off on WebGL,
and Cranpose's renderer uses none of them. Without the call an adapter asks
for every extension, as upstream's does, so other users of the fork, such
as a game engine making its own device, keep them all. On the Mate the
page's extension asks went from 12-24 ms to under 2 ms.

### GL programs are read when a submission first needs one

WebGL answers a program query only once the GPU process has run every
command sent before it, and a phone's Chrome links one program at a time.
Upstream's GL device reads each program's link as it makes the pipeline,
so making the pipelines waits for every link. The fork's device sends the
link and leaves it unread; a submission reads the links of the programs it
uses before it replays its commands, so the read waits for the links alone
and not for the draws sent before them. Once a submission has waited for
a read, it also reads every other unread link: the GPU process has run
those links too, so each costs only its reply, where a later frame's first
use would wait for every frame sent before it. In Chrome on a Huawei Mate
20 X a gauntlet frame early after the first waited 12-43 ms for the one
program the first frame did not draw with; with the read in the first
frame, frames two to five took 25-30 ms less in all.

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

### A WebGPU frame can learn its pipelines while they compile together

On the WebGPU backend `createRenderPipeline` hands the browser each
pipeline as it is created, and Chrome's GPU process compiles them one after
another on its main thread, presenting nothing meanwhile. On a first visit
to the gauntlet on an Apple M5, with no Metal shader cached, the twelve
pipelines of the first frame held it for 0.85 to 0.96 s; created through
`createRenderPipelineAsync` the same twelve compiled side by side in 0.21
to 0.25 s, and creating them again with `createRenderPipeline` afterwards
took 3 ms in all, since the browser hands back the one it compiled.

`Device::begin_pipeline_discovery` starts that on a WebGPU device: every
render pipeline created until `Device::end_pipeline_discovery` goes to
`createRenderPipelineAsync` and never draws, and every pass or bundle begun
meanwhile records no draw. The future `end_pipeline_discovery` returns
resolves, with the compiled pipelines to keep, once all of them compiled or
failed. Cranpose's web shell draws its first frame this way, then installs
a new renderer and draws the frame again once they compiled. On the M5
the gauntlet's first frame showed after 0.5 to 0.75 s instead of 1.0 to
1.25 s, the same to the pixel.

### WebGPU pipelines compile in the background after the first frame

A pipeline the page creates after its first frame went to the browser's
`createRenderPipeline`, which Chrome compiles in its GPU process one after
another while nothing it is given draws. On an Apple M5 with no Metal shader
cached, the 19 pipelines the desktop demo's Liquid UI tab first needed held
every frame of the page 1.4 s, and another 0.2 s after.

`Device::compile_pipelines_in_background` sends every render pipeline the
device creates from then on to `createRenderPipelineAsync`. The pipeline
object exists at once; its draws are left out until the browser compiled
it, as a pipeline discovery leaves them out, and from then on it draws.
`Device::take_background_pipelines` returns a future for the pipelines
created since the last call, which a renderer awaits to drop what it drew
without them and draw again. Cranpose's web shell turns it on after the
first frame, whose pipelines discovery compiles.

`RenderPipeline::is_ready` tells whether a pipeline's draws draw: `false`
while the browser compiles it in the background and for one pipeline
discovery created, `true` on every other backend. Cranpose's renderer draws
with a general pipeline that already compiled until a specialized one is
ready.

### A surface configures while another thread submits

`Surface::configure` waits for every submission made before the call, then
failed with `GpuWaitTimeout` when the queue was not empty afterwards. The
queue is not empty when a game engine renders on its own thread with
Cranpose's device: Bevy's render thread submitted while Cranpose resized its
window, and the error stopped Bevy's renderer. Work submitted after the wait
cannot use the surface's textures, since none is acquired and a presented one
cannot be submitted, so `wgpu-core` accepts both results of the wait, as
upstream's trunk does.

### Safari gets WGSL without texture and sampler `let`s

Safari 26 rejects WGSL that names a texture or a sampler with `let`, such as
`let texture = base_color_texture;`, with "'texture_2d<f32>' cannot be used as
the type of a 'let'". That is the `texture_and_sampler_let` language feature,
which Chrome has and WebKit does not. Bevy 0.20's PBR shaders use it in 32
places, so on an iPhone every material with a texture failed to build its
pipeline and was not drawn.

The `portable-wgsl` feature reads, on a WebGPU device whose browser does not
list the feature in `navigator.gpu.wgslLanguageFeatures`, every WGSL module
that has such a `let` with naga and writes it back without the names, so
the texture or sampler is written where the name was used. A browser that
has the feature gets the source as it is. The feature adds naga's WGSL
reader and writer to a wasm build, so only crates that bring such shaders
turn it on: `cranpose-bevy` does on the web.

### GL contexts keep the 60 varying components the standard grants

Brave adds noise to WebGL parameters against fingerprinting, so on some sites
it reports 59 for `MAX_VARYING_COMPONENTS`, below the 60 that GL 3.3, GLES 3.0
and WebGL2 guarantee. `wgpu-hal` divided that by four, so the device allowed
14 inter-stage variables, and a pipeline with 15, such as Cranpose's shape
pipeline, failed validation on a GPU that runs it. Cranpose's HUD over a Bevy
game drew nothing in Brave on a Mate 20 X on github.io, while the same page on
localhost, where the noise left 60, drew it. The GL adapter now takes at least
60 components.

### Vulkan pipelines let their shader modules go

`wgpu-core` keeps every shader module a render pipeline was made from for
as long as the pipeline lives, and a `wgpu-hal` module made from WGSL or
naga IR keeps that IR, from which it writes SPIR-V when each pipeline is
made. Cranpose specializes and folds each stage of a pipeline into a
module of its own, so every pipeline it kept held the IR of both its
stages. A Vulkan pipeline needs none of its modules once it is made: the
`VkShaderModule`s written for it are destroyed at once. On Vulkan the
pipeline now lets its modules go, and the caller's handles alone keep
them.

On a Mate 20 X the gauntlet at tier 12 held 78.0 to 78.2 MB of native heap
on a second start instead of 81.4 to 82.2 (PSS 149.8 instead of 153.3 MB),
and 1.8 to 2.0 MB less on a first start (three launches each, paired).
Other backends keep the modules: their pipelines may need them.

### Swapchain images come back as they presented

`wgpu-core` hands out every acquired surface texture as uninitialized, so
a pass that loads it gets a clear first, and a frame must draw the whole
window. Android's swapchain gives back each buffer with the pixels it
presented. Cranpose's renderer draws into such a buffer only what changed
since the frame it holds.

`Surface::keep_presented_images(true)` asks for that. `wgpu-core` then
counts, per swapchain image, whether it presented since the surface was
configured; such an image is handed out initialized and in the layout it
presented in, so a pass that loads it reads its pixels. A failed present or
a discard forgets the image. `SurfaceTexture::image` gives the image's
index and whether it is kept, on Vulkan; other backends give `None`.

A frame that draws nothing new into a kept image submits no work. Vulkan's
present waits on the image's acquire semaphore only through a submission,
so the queue's present now submits once when no submission waited on it
yet.

Vulkan does not promise that a presented image keeps its pixels, so only
Android asks for it. On a Mate 20 X (Mali-G76, 4 images, FIFO) every frame
of a probe that drew a part of the window into a kept image read back equal
to the frame drawn whole (36 of 36).

### Update the fork

1. Copy the new upstream versions' crates.io sources over the crate
   directories (`src`, `build.rs`, `Cargo.toml`, licences, readme), as one
   commit.
2. Rename the packages and point the forked dependencies at their sibling
   directories (`package = "cranpose-..."`, `path = "../..."`), as before.
3. Reapply the patch, or drop the fork once upstream ships the fix.
