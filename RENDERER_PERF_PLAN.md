# Renderer Performance Plan

Goal: every robot/perf scenario must sustain at least 120 FPS, including the heavy LeetCodeDaily layout scroll path.

## Ground Rules

- Measure before changing architecture.
- Commit each verified improvement as a coherent checkpoint.
- A checkpoint is verified only after formatting, tests, clippy, Android release, wasm build, and the full robot suite pass.
- Prefer root-cause renderer and invalidation fixes over local workarounds.
- Keep the repo in a complete state after every commit.

## Loop

1. Run the available perf scripts and record the slowest scenarios.
2. Add focused telemetry when the scripts do not expose the bottleneck.
3. Rank bottlenecks by measured frame time, not intuition.
4. Implement the highest-impact architectural fix.
5. Run focused tests for the touched path.
6. Run the full verification gate.
7. Commit the verified improvement.
8. Repeat until the minimum measured FPS is at least 120 everywhere.

## Current Suspicions To Prove Or Reject

- Scene rebuild and render-graph traversal still occur too often during scroll.
- Text layout, glyph cache misses, or text upload churn dominate heavy list screens.
- Layer/effect segmentation creates too many render passes in mixed content.
- Upload staging or buffer allocation still causes avoidable CPU/GPU synchronization.
- Lazy-list draw invalidation may still rebuild more scene graph than required.

## Measurement Targets

- Hacker News scroll.
- LeetCodeDaily full layout scroll stability.
- Markdown and text exact-scroll contracts.
- Robot perf harness.
- Lazy-list perf validation.

## Status

- Checkpoint `008d630c` committed the first renderer draw-update cleanup.
- `perf_robot_cpu.sh`/symbol-heavy profiling is not safe on this no-swap machine; it caused an OOM reboot. Use stripped `release-fast` robot binaries for iteration, then full verification before commit.
- Built-in robot perf harness after the current renderer changes:
  - `lazy_list_scroll`: 1552.2 FPS.
  - `text_heavy_scroll`: 1682.8 FPS.
  - `backdrop_blur`: 1793.0 FPS.
  - `opaque_scene`: 6505.5 FPS.
- Heavy LeetCodeDaily full-layout scroll:
  - Baseline with redundant identity alpha mask still active: 98.5 FPS, 66 blur passes/sample, 135 offscreen acquires/sample.
  - After identity alpha-mask no-op: 103.9 FPS, 66 blur passes/sample, 134 offscreen acquires/sample.
  - After translation-invariant shape-shadow surface cache: 196.1 FPS, 0 blur passes/sample, 1 offscreen acquire/sample.
- The current highest remaining renderer bottleneck is composite submit count. LeetCodeDaily still reports 175 submits/sample because cached offscreen composites use immediate command submissions. The proper next renderer architecture work is a batched/dynamic-uniform composite path, not a demo-specific shortcut.

## Current Checkpoint Scope

- Make identity rounded alpha masks a no-op at modifier construction time.
- Add an opt-in LeetCodeDaily perf probe without changing the demo layout/content.
- Share robot perf render-stat accumulation across perf examples.
- Cache translated, text-free shape-shadow surfaces by normalized content hash, pixel size, scale, and blur radius.
- Batch shape-only shadow miss rendering into one encoder before caching the blurred surface.

## Current plan: the 34.8 → 60 FPS path on the watch

The measurements below identify the only credible large gain, and the work is
scoped to it. Anything not named here is not authorized by current evidence.

The watch measurements already identify the only credible large gain:

| Stage | p95 |
|---|---:|
| Producer scene work | 11.85 ms |
| Present/render work | 12.58 ms |
| 60 FPS budget | 16.67 ms |

Both stages fit independently. They miss 60 because they still execute serially.
The target architecture is therefore fixed:

```text
producer/UI thread: frame N+1 update, record, verify, lower, publish
present thread:     frame N prepare, acquire late, encode, submit, present
```

Steady-state throughput must become `max(producer, present)`, not their sum.
Do not do more recorder, arc, pipeline-specialization, texture-rotation, mailbox,
or whole-graph `Rc`/`Arc` work before this is measured on the watch.

## 1. Finish the complete packet boundary

Land the current step 6b so every root and the dev overlay are producer-lowered
into an owned `FramePacket`. `GpuRenderer` must consume packets only. It must not
read the graph, text-layout state, or producer `AppContext`.

Remove the `app_context.enter(|| gpu_renderer.render(...))` wrapper from the
backend call. `RendererFrontend::build_frame_packet` may enter its producer
context; packet consumption must work with no current `AppContext`.

Do not use Mac FPS as the 6b gate. The recent 35–58 FPS host spread reproduces on
the same code and is bootstrap/game-phase variance. Gate 6b on identical packet
content, pixels, replay counts, and direct/surface/overlay behavior.

## 2. Make in-flight packets cancellable before adding the thread

The current replay-generation mismatch path drops `ReplayFrameOps` and then
continues rendering the packet. That is unsafe once device loss or renderer
replacement can leave the packet's `RetainedDraw` slot ids pointing into a dead
store. A missing slot currently draws nothing.

Add explicit packet validity:

```rust
struct FramePacket {
    frame_id: u64,
    renderer_epoch: u64,
    surface_epoch: u64,
    viewport: (u32, u32),
    root_scale: f32,
    // owned root, overlay, replay ops
}

enum PresentOutcome {
    Presented,
    Cancelled(CancelReason),
}

struct RenderReturns {
    frame_id: u64,
    outcome: PresentOutcome,
    // replay ack and recyclable buffers
}
```

Validate the epochs and viewport before applying releases or encoding anything.
A mismatch cancels the entire packet and returns all buffers. Device loss,
surface replacement, resize, and root-scale changes cancel queued packets,
advance the appropriate epoch, and force producer reconstruction.

Required protocol tests are limited to the cases that can corrupt the pipeline:

- renderer replacement with one packet rendering and one waiting;
- resize/root-scale change with a packet waiting;
- a cancelled packet returns its scene and replay buffers;
- frame N retained slots cannot be released or reused before N completes;
- the producer applies N's acknowledgement before planning N+2.

## 3. Split actual ownership, not just types

The Android UI thread keeps `RendererFrontend`, the scene graph, layout state,
input, and animation state. The present thread exclusively owns `GpuRenderer`,
surface configuration, surface acquisition, GPU caches, replay GPU slots,
encoding, submission, and `present()`.

`GpuRenderer` contains thread-confined `Rc` caches, so construct it on the
present thread instead of constructing it on the UI thread and trying to move
it. Send only owned initialization data such as device/queue handles, surface
format, backend capabilities, and fonts.

Remove producer reads of backend internals:

- publish immutable `replay_supported` capability when the backend starts;
- publish `needs_frame_warmup` through a small atomic/status snapshot;
- return stats and errors in present-thread messages.

The graph and `DrawPrimitive` never cross the boundary.

## 4. Implement the Android depth-one runtime

Use one FIFO packet slot: at most one packet rendering and one waiting.

Producer flow:

1. Process input and lifecycle events.
2. Drain `RenderReturns`; apply the replay acknowledgement and recycle buffers.
3. Obtain packet credit before running the next visual update.
4. Run `shell.update()`, lower the complete frame, and publish it.
5. Return immediately to the Android looper.

Present flow:

1. Receive the next packet FIFO and validate epochs.
2. Apply replay operations and prepare CPU-side GPU batches.
3. Acquire the surface late, after preparation.
4. Encode, submit, present, and return the outcome/ack/buffers.
5. Wake the Android looper to permit the next producer frame.

Do not build a packet and then discard or overwrite it when the queue is full.
Do not block the input thread on surface acquisition or presentation. Gate frame
production on queue credit so backpressure happens before expensive update and
lowering work.

Surface resize, pause/resume, surface destruction, reconstruction, OOM, and
shutdown are typed control messages. The present thread acknowledges an
invalidation before the producer publishes under the new epoch.

Wasm keeps the same build/consume packet APIs and calls them synchronously.

## 5. Measure the first real pipeline on the watch

The first watch build after the Android runtime lands must report, without
per-frame diagnostic logging:

- presented FPS and frame-present intervals;
- producer p50/p95/p99;
- present p50/p95/p99;
- surface-acquire and present waits;
- shared executor queue delay and contention per stage;
- CPU and matched-start-temperature thermal behavior;
- cancellations, replay-generation drops, fallback, and rematerialization.

`Stage::Present` currently has no production caller, and fine producer chunking
is only a latency heuristic. Do not redesign the executor speculatively. If the
overlapped measurement pushes either stage beyond 16.67 ms, first use the stage
queue-delay evidence:

1. reserve enough executor capacity for present work or add real bounded
   present priority when contention is the cause;
2. bypass repeated hashing only for ranges proven volatile when producer work
   is the cause;
3. profile and remove the specific render p99 source when present work itself
   is the cause.

No other optimization is authorized by the current evidence.

## Success condition

Success is repeated, matched-temperature Pixel Watch runs sustaining
approximately 59.5–60 **presented** FPS, without alternate-vsync cadence, visual
differences, packet cancellation loops, or thermal decay. Internal loop FPS and
Mac FPS do not count.
