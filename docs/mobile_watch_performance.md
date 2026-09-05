# Mobile and watch frame budget

The acceptance target is sustained display-rate rendering (at least 60 fps on
these 60 Hz displays) in cranorbit MEGA BOSS and cranpose-showcase on the Huawei
Mate 20 X and connected watches, with no image-quality reduction against main.
Only Cranpose implementation changes are in scope. App source, effect parameters,
resolution, animation work and scene complexity stay fixed.

## Source and measurement isolation

`perf/mobile-watch-60fps` is in `Cranpose-mobile-watch-60fps`, separate from the
concurrent renderer checkout. It started at `d4f38bb3`, then incorporated the
renderer commits `ffde4bf3`, `5204280b`, `4d80a9d0` and `90e14d68`. Main is
`0d195313`, already an ancestor.
The concurrent checkout's uncommitted files are never build inputs.

App copies pin cranorbit `0334e16` and showcase `4ad4080`. Cargo overrides select
the Cranpose checkout without changing either app. Android builds use the app's
release feature set and the fully optimized `release` profile. Watch prototypes
replace the ARMv7 native library in the same bench-package APK. Main and renderer
baseline APKs were built on samarch-1; column prototypes were built on macm3.
The measurement APK is `com.dmitry.orbitbreaker.bench`; store-app data is untouched.

The watch is a Pixel Watch 3, 408 by 408, ARMv7. The phone is a Huawei Mate 20 X,
ARM64. Each arena run starts MEGA BOSS through its existing launch extra, warms up
for eight seconds, then counts the application's SurfaceFlinger presents for
60 seconds. Diagnostic runs are separate: extensive per-frame logging costs
throughput. Battery temperature, thermal status, foreground activity, frame
telemetry and a nonblack screenshot accompany each run. Thermal overrides are
never used. The game progresses during each window, so intervals must match;
a rising late-window fps is not evidence of a renderer improvement by itself.

Two runs were invalidated because the concurrent showcase task took the watch
foreground: `watch-columns-diagnostic` and `watch-baseline-b`. The measurement
harness rejects foreground loss, process replacement and multiple app surfaces.
Their display counts are excluded below.

## Record delivery experiments

| Watch MEGA BOSS, 60-second window | Display fps | Notes |
| --- | ---: | --- |
| Renderer baseline, first run | 48.73 | 33.4 to 36.5 C |
| Baseline with present thread | 49.91 | No substantial gain |
| Unchanged 112-byte records as instance attributes | 49.22 | No substantial gain |
| Main | 42.99 | Variable run; insufficient replication for an ordering claim |
| Columns packed on render thread | 41.86 | Extra conversion increased render time |
| Columns written by recorder | 47.82 | 34.5 to 37.2 C; thermal status 0 throughout |
| Renderer baseline, matching follow-up | 48.17 | Foreground verified |
| Recorder columns, full-buffer upload ablation | 50.44 | Three writes instead of about 41 |
| Recorder columns with pooled staging | 48.50 | 34.3 to 37.1 C; thermal status 0 |
| Recorder columns with pooled staging and present thread | 52.45 | 34.9 to 37.7 C; thermal status 0 to 1 |

The Huawei arena presents at 60.06 fps with both the baseline and unchanged
record attributes. The 60 fps acceptance target remains unmet on the watch.

Native instance attributes alone leave the watch's GPU pass unchanged at about
21 ms under diagnostics. This closes the attribute-fetch hypothesis on its own.
The columns expose independent 64-byte body and 32-byte curve buffers; the
16 bytes needed only to reconstruct original arc arguments remain on the CPU.
A rotation updates the curve column without resending unchanged body data.

Packing columns after recording reduced transferred bytes but added an entire
conversion and retained another copy. That representation was rejected. The
recorder writes the columns directly; `ShapeRecord` is reconstructed only for
consumers that need the complete logical value. Stored runs retain the original
`Arc<RecordTables>` and upload its slices directly. The arena copies the GPU
columns while rebasing brush and placement indices. CPU storage still preserves
every original bit, including signed zero and NaN payloads.

The next measurement explains why bytes alone were insufficient. Diagnostic
baseline uploads were about 1.90 MB in two queue writes; recorder columns were
about 0.90 MB in 41 writes. Upload time was 4.37 versus 6.03 ms, respectively,
although those diagnostic windows differ in thermal state. Removing fragmented
writes sent 1.63 MB in three writes at 4.80 ms and improved the non-diagnostic
run to 50.44 fps. The full-buffer ablation is not a shipping policy: it discards
the benefit for sparse changes. Pooled staging retains the sparse updates and records their copies into the
frame encoder. A standalone Adreno 702 upload probe, alternated twice, reduced
40 scattered writes from 2.60 ms to 1.08–1.15 ms. A single 1.9 MB write stays
at 1.49–1.65 ms with either allocator. The whole-app results above remain below
60 fps, and the 52.45 fps result requires a matching threaded baseline repeat.

## Correctness evidence

The unchanged-attribute experiment matches all seven record-path captures byte
for byte on the same Linux adapter. Swapping rectangle and colour attributes
makes the image test fail. The rotating-record test fails on the starting
renderer because a rotation uploads the complete record, then passes with
columns. Deliberately suppressing curve updates makes its motion assertion fail.
It also compares an updated retained run with a fresh renderer and checks paint
invalidation. Original-argument preservation is tested byte for byte; deliberately
zeroing the original arc arguments makes that test fail.

Graphics, render-common and renderer unit tests, record-path images, rotating
uploads, stored-run uploads and variant parity pass on macm3. Workspace Clippy
passes with no warnings. The release web build passes its size gate at
12,077,735 bytes against 14,680,064. Full workspace validation exposed a host
signing dependency in xtask's temporary Git fixtures; the fixture command disables
commit signing locally, with a regression test proven red first. The Linux shadow/capture failure reproduced on both unmodified renderer bases.
Its identity-shader fixture bilinearly sampled half-float values, adding one-byte
rounding on lavapipe. The first capture was exact; the second introduced the
rounding. Replacing only that fixture’s sample with an explicit texel load
restores strict byte equality without changing runtime shaders or tolerances.

The staging lifecycle test discards an encoder, reuses mapped storage across
24 frames, and reads back sparse writes at three offsets. Deliberately writing
every copy at offset zero fails it. Stored-run metadata is invalidated whenever
a frame does not submit, including a successful frame that declares no passes. These checks do not establish the device frame-rate target.


## Ordered tile feasibility

An external probe compares 64 overlapping layers of pseudorandom RGBA over
4,096 pixels on the watch. Rounding only the accumulated destination differs
in 2,998 of 16,384 bytes, by up to two levels. Quantizing the source RGBA to
8 bits before each blend, then quantizing the destination, matches all bytes.
The intentionally incorrect accumulation supplies the red correctness case.
This establishes one attachment rule; it does not establish shape coverage
parity or an application performance gain. The probe is outside the repository.

The first dense prototype uses the captured command’s first 15,007 solid arcs,
with the existing shape shader as raster reference. Ordered pixel evaluation
remains slower: 4-pixel tiles take 114 ms with global record reads, then 75 ms
with workgroup-shared record batches; the respective raster controls take 41
and 28 ms. These are isolated submission/fence timings, not app fps, and their
variation rules out comparing the two controls as a speed improvement. Both
compute outputs differ in 18,449 of 665,856 bytes, by at most two levels.
Neither variant meets the correctness or performance requirements.

The full workspace tests and Clippy pass on Linux after updating the ownership
guard to recognize frame-graph submodules. The corrected release web build also
passes without warnings. `just android` passes. The robot build exposed two
removed scratch-buffer names in its memory reporter; it now reports arena
staging bytes, stored-run bytes and run count from the existing diagnostics API.
The complete robot gate is being rerun.


GPU vertex expansion also stays outside the branch. Its first prototype stopped
before the final records when it used a runtime buffer-array length as the draw
bound; using the explicit active record count restores exact equality over all
665,856 bytes. One invocation per record emits its six original vertices and
preserves both coverage and hardware blending. However, alternating against the
raster reference rejects the speed claim: the first raster control takes 41 ms,
then the warmed controls take 14.4–16.1 ms against expansion’s 19.1–19.3 ms.
The cached expanded draw alone takes 15.7 ms. The early 28–41 ms controls were
cold-GPU measurements and cannot establish a benefit.

## CPU recording

An Android app-profileable `simpleperf` capture attributes 10.5% of all CPU
cycles to `push_arc_band` itself, 10.7% to `push_shape`, 6.5% to `sincosf`,
and 4.5% to arc normalization. The watch runs the ARMv7 library. Removing
the intermediate function boundary alone does not help: alternating recording
probes remain at 7.54–7.60 ms for 15,161 captured arcs.

Removing arc trigonometry in an external, deliberately incorrect probe lowers
that recording cost from 7.53 to 5.14 ms and changes the record fingerprint.
The implementation retains the most recent exact input and result independently
for midpoint and half-sweep trigonometry. Float bit keys preserve signed zero;
changing either angle invalidates its result. It holds two entries per recorder,
allocates nothing, and retains the same arithmetic and full-circle sentinel.
Alternating ARMv7 probes take 7.565/7.567 ms without reuse and 6.833/6.856 ms
with reuse, with the same complete-record fingerprint. Deliberately accepting
a cache entry after its angle changes fails the bit-preservation unit test.
After restoring the key check, all 215 graphics tests pass.

The latest 4d80 app comparison is not an acceptance result: the baseline watch
run reaches thermal status 2, and the following pooled run reaches status 3
and drops to about 39 fps. Their 49.76 and 46.56 averages compare different
thermal conditions. Huawei presents 3,628 frames over 60.516 seconds (59.95 fps)
with the pooled build. Android 10 names that activity layer without Android 15's
`VRI-` prefix; the harness now recognizes both exact package prefixes.

## Pixel coordinates and short strips

A one-segment strip covers a short arc with four vertices instead of six.
The initial watch probe saves about 10% of the warmed dense draw, but differs
in 22,135 output bytes by up to two levels. Removing reconstruction through
interpolated UV coordinates eliminates the dependence on tessellation: the
watch probe then matches all 665,856 bytes between one and two segments.
The underlying issue is evaluating an analytic shape at a coordinate perturbed
by triangle interpolation instead of at the device pixel centre.

The shape shader evaluates coverage and gradients at the fragment position plus
the viewport origin. It no longer computes or carries UVs. This is valid for
the record path: record vertices and their shape parameters share device space;
layer transformations happen in the compositor. The existing seven image
goldens and three specialization parity tests pass on Metal without changing
their images or tolerances. A new test draws clipped arcs with all three caps,
a fractional root scale and viewport origin, and a rectangle mixed into the
recording. Its one-, two-, four- and eight-segment outputs must match exactly.
The test fails on the interpolated-coordinate shader and passes on device
coordinates. Strip coverage tests still require every shaded pixel to lie
inside the geometry.

Short arcs can consequently share the one-segment class with rectangles.
The shader no longer specializes on strip length. Pipeline keys contain paint
and shading facts; each draw carries its index-buffer class independently.
Deliberately merging adjacent draws of different classes fails the index-range
regression test; restoring the class check passes it.

The first full robot run passed 157 tests and failed seven, with two skipped
because the physical display was asleep at discovery. All six external-input
failures and both skipped cases pass on isolated X displays. The remaining
glass warm/cold comparison repeats under the exact software-capture recipe:
1,029 pixels differ at the fifth scroll step, concentrated on the bar's button.
The dependency investigation and fix are documented below. These checks do not
establish the application frame-rate target.


## Dependent backdrop invalidation

The glass scroll robot fails identically on the clean renderer baseline with
shader specialization disabled. Removing layer-cache reads makes every step
pass. The leading cause is a missing dependency: `take_uncached` keys every
queued backdrop before any stage resolves, so a later glass cannot hash the
output of an earlier queued glass. A focused two-glass test confirms this:
changing only the lower blur leaves all 1,536 interior pixels of the upper
glass different from an uncached render (maximum channel difference 197).

Keying each stage after earlier stages resolve preserves the existing cache
and captures the dependency through `SourceContent`. Turning off caching would
avoid reuse but discard valid results; expanding hashes over unresolved effects
would duplicate the renderer's dependency evaluation. Stage-ordered keying uses
the resolved inputs already required for drawing.


Stage-ordered keys pass the focused regression and the original ten-step glass
scroll robot, with its pixels and tolerance unchanged. A nested cache chain
settles from the bottom upward, two admission frames per stage. The existing
cache-performance fixture now warms for twice its visible row count plus its
overlay, rather than four frames. Its measured assertions remain unchanged:
zero misses and zero blurs when still, every still row cached under animation,
and reuse during rigid scrolling. A 20-frame diagnostic first confirmed that
these contracts still hold after the dependency fix.

The latest full-minute device results are 52.19 fps for watch Megaboss with
threaded presentation, 59.96 fps for Huawei Megaboss, 23.47 fps for watch
Showcase and 23.03 fps for Huawei Showcase. These use the short-strip build
before the dependent-cache fix. Two earlier watch trials were invalid: another
application took the foreground in one, and Android's full-backup service
terminated the benchmark in the other. Neither counts toward acceptance.


The append probe separates the remaining CPU costs. Moving the column append
out of line slows the exact workload from 6.75 ms to 7.41 ms, with the same
fingerprint, so it is rejected. Removing bounds and segment bookkeeping saves
only about 0.47 ms and changes the fingerprint. Removing record construction
and append altogether drops 6.75 ms to 4.24 ms. Both removals are diagnostic
ablations in an external graphics copy; neither is a production candidate.


The supported `thumbv7neon-linux-androideabi` target preserves the record
fingerprint but measures 6.73–6.74 ms against 6.74 ms for the current ARMv7 target:
no material benefit. No build-target change is justified by this workload.
A persistent-worker prototype splits the same 15,161 arcs in order. One worker
measures 7.05–7.08 ms, two 3.92–3.93 ms, and three 2.85–2.86 ms. Every materialized
record matches the serial reference. This is an upper-bound feasibility result:
it does not yet include main-thread raw input recording, ordered segment
coalescing, or concurrent application/rendering work. Production changes need
a recording/preparation boundary and a complete byte/order regression test;
parallelizing application draw closures would be wrong because they read UI
state on the UI thread. The WebAssembly path must use the same preparation
kernel serially.

Copying owned brush-bearing inputs and joining the workers' output columns
erases that gain: two workers take 6.88–8.49 ms and three take 7.17–7.73 ms.
Using 48-byte copyable inputs and retaining the prepared chunks measures
4.58–4.72 ms with two workers and about 3.93 ms with three. This includes input
copying and checks complete records in order, but still excludes application
execution and global segment coalescing. A production design must avoid the
failed prototype's final copy while preserving immutable scene snapshots.

The short-strip, column and staging implementation passes workspace tests,
Clippy, precommit, release web and Android builds. The exact robot recipes pass
162 GPU tests and all four software capture tests, with no failures or skips.
Software presentation measurements are not device FPS evidence.

Preparing existing columns in place on Rayon workers preserves the complete
workload fingerprint, but takes 6.41–6.44 ms against 6.74–6.77 ms for serial
recording. Its 90th percentile rises to 8.50–8.69 ms from 6.83–6.93 ms. Most column
construction still runs on the caller, and the worker boundary adds scheduling
variance. This version is rejected; its source and patch remain outside the
branch. The experiment's mixed-record test also catches stale command identity
after appending to a fingerprinted recording. All three append lanes invalidate
that fingerprint, with the regression proven red before the fix.

## Glass split integration

The committed substrate and glass split work from `90e14d68` is incorporated.
The preceding cache-correct build measures 23.40 fps on watch Showcase and
22.81 fps on Huawei Showcase. Threaded presentation leaves the watch at
23.51 fps. The combined substrate build measures 19.23 fps on Huawei; the
first watch run loses the foreground and is excluded. These figures do not
establish a gain across both GPUs.

The full combined test suite exposes one channel value differing by one at
pixel (103, 263) in the glass specialization comparison on lavapipe. The clean
`90e14d68` baseline repeats the same failure, while Metal passes. Removing
the split draws on that baseline restores exact parity. Removing only the
bevel screen blend also restores it. At zero bevel strength, the expression
`1 - (1 - rgb) * (1 - bevel)` can round through two subtractions, while the
compiled interior folds it to `rgb`. Expressing the same screen blend as
`rgb + (1 - rgb) * bevel` preserves its zero-strength identity in both paths.
The split draws remain enabled, and the strict Linux parity test passes.

Review also finds the compiled-pipeline key omits the split override's name.
Two shaders with identical source and different split names consequently share
the first pipeline. A focused probe makes the second shader render red instead
of blue. Including the actual split name and variant in the key passes that
regression and all twelve backdrop-atlas tests on Metal.


A valid repeat of the combined substrate build gives 23.51 fps on watch
Showcase; Huawei Megaboss remains at 59.95 fps. Workspace tests, Clippy,
release web, Android, all 162 GPU robot tests and all four capture robot tests
pass after the two glass correctness fixes.

Specializing full material activity and removing its zero-weight interior
plain-backdrop read preserves exact pixels across nine activity/rim cases on
Metal. Deliberately removing the read for resting material fails 3,692 bytes
with a maximum difference of 128. A sharp background hid that mutant because
SrcOver reconstructed the destination; the regression uses a blurred capture.
The candidate measures 23.50 fps on watch Showcase and 19.96 fps on Huawei,
which establishes no useful watch gain. Its production change is set aside;
the activity parity regression remains.

Moving normalized arc geometry into copied worker inputs takes 4.46–5.16 ms
with three workers, but its 90th percentile varies from 6.91 to 11.84 ms.
The two-worker path takes 6.83–7.18 ms and also worsens tails. This external
prototype does not establish a production gain or replace the serial recorder.

## Frame arena integration

The branch includes `a4013903` from the renderer work. Frame uniform, image and
shape uploads retain its arenas; body and curve columns bind as instance vertex
buffers, while brushes, stops and placements use three dynamic table offsets.
Stored recordings keep sparse pooled copies. A frame's shape arena uploads each
nonempty column once; the warm glass fixture uses five writes with separate
body and curve columns.

Metal passes the arc tessellation, rotating upload, backdrop parity, glass cache
and record golden tests. Deliberately binding every arena's vertex columns at
zero makes the shadow golden empty and changes 20,494 painted-layer bytes by
more than two levels, with a worst difference of 208. Restoring each column's
actual chunk offset passes all seven goldens.

The admission budget regression uses nine independent glasses. Overlapping rows
form a dependency chain once capture keys include resolved lower stages, so they
cannot test simultaneous admission. The independent fixture checks admission
progress, the per-frame limit, eventual cache hits and cached/reference pixels.
Removing the pixel budget makes it fail with nine admissions in one frame;
restoring the budget passes.

The watch list-scroll A/B/A/B uses the same 36,300 to 60,80 swipe and reverse,
16 warm-up swipes and a full-minute SurfaceFlinger window per leg. All four
runs keep the benchmark's PID and foreground stable. The APK before arenas
measures 26.66 then 19.08 fps; the integrated arenas measure 37.09 then 24.50 fps.
Starting/ending battery temperatures are 36.5/38.6, 38.6/40.1, 41.0/41.3 and
41.1/41.1 degrees Celsius, respectively. The gain repeats, but these are not
cooled steady-state results and do not demonstrate sustained 60 fps.

The shared-channel glass sampling guard is byte-exact on Metal and lavapipe:
channels whose lens ramp is fully clamped can reuse the green sample. Forcing
the guard everywhere fails 912 bytes in the activity parity test, maximum two.
It improved the preceding Huawei substrate build in individual runs, but the
combined arena A/B/A/B is 23.60/23.53/23.58/23.52 fps. It establishes no gain on
this renderer and is not retained. The diagnostic patch and APKs stay outside
production.

Upload ring review catches missing reclamation after a large frame. A GPU test
forces three buffer generations, reads back each generation's distinct bytes,
then asks a small frame to release the oversized allocation. The original ring
fails that last assertion. Reclaiming when nonzero frame workload drops below
one quarter of retained capacity restores the allocator's reclamation behavior;
all generation bytes remain correct. Empty reset calls do not trigger shrinking.
The source-ownership contract checks frame upload ranges and the behavioral test
covers growth and reclamation directly.

On lavapipe, the independent-glass fixture has three pixels with two channels
each differing by one output level after caching. Its comparison uses the same
maximum per-channel bound as the existing glass-layer cache regression, rather
than summing channels; shader specialization tests still require exact bytes.

The independent upload GPU probes expose a Linux headless setup race when both
create devices concurrently under the default adapter: the process receives
SIGSEGV. The same tests pass with one test thread and under lavapipe. A shared
device fixture serializes these two probes, following the integration suite's
existing GPU fixture ownership; production renderer scheduling is unchanged.

The timing-report capability probe also initialized an adapter outside the GPU
test lock; it takes that lock before probing. `LockedRenderer` releases its lock
last, after its renderer and app context, so another test cannot initialize a GPU
while the previous one is still tearing down resources.

The default Intel UHD 730 adapter repeats the packed-glass test's one-level
red/green channel difference on clean `90e14d68`, at the same pixel and with the
same byte values as this branch. The fixture now applies its documented mapping
allowance per channel, using the cache regression's shared maximum-channel
helper. The numeric allowance stays one, zero-tolerance blur cases stay exact,
and shader specialization still requires exact byte equality.

### Texture-pool integration checkpoint

Integrated renderer `f0008069` with `78b24d87`. The release-age texture pool,
its allocation and exact reused-pixel tests, and the frame resource counters
are retained. Ring reclamation has one shared predicate and both its unit
boundary checks and the GPU growth/shrink readback test. The Intel headless
locks and per-channel atlas assertions remain. Metal validation passed all
134 renderer unit tests, 15 atlas parity tests, four glass-cache tests, 21
architecture contracts, and both transient-pool tests. Precommit passed.

The preceding `78b24d87` checkpoint passed all 162 GPU robot tests; capture
robots are queued behind the shared host lock. This result does not establish
60 FPS on a physical device.

The full-minute `watch-orbit-arena-team` measurement is 31.061846 FPS over
60.8785 seconds, with stable process and foreground. Battery temperature was
40.5 C before launch, 40.9 C at measurement start, and 42.4 C at completion.
Windowed update medians grew from 16.87 to around 30 ms; render medians grew
from 3.96 to 6.79 ms. It is a valid hot-device sample, not a comparison with
the earlier cooler 52.19 FPS reading.
