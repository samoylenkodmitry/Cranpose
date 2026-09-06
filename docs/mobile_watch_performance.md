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

### Rejected queued shape preparation

The complete experiment queued solid rectangles, rounded rectangles and arcs,
prepared large native batches in a bounded Rayon pool, retained independent
column chunks for direct upload, and rebuilt global segment metadata without
joining the output buffers. Small batches and WebAssembly used the same scalar
kernel. It is stashed; none of that implementation or its dependency remains
in the performance branch. The large public-record and retained-GPU-run tests
remain useful independently.

The public DrawScope benchmark records 15,161 arcs with changing angles through
reused storage. Both apps were stopped for isolated CPU measurements. Every
valid variant produced fingerprint `817059c723455dd8`. Milliseconds, p50:

| Device | Baseline | Worker pool | Scalar queue | Two workers |
| --- | --- | --- | --- | --- |
| Huawei | 1.840 / 1.846 | 6.328 / 7.179 | 2.603 / 2.610 | 11.808 / 11.848 |
| Watch | 7.521 / 7.520 | 8.138 / 10.201 | 10.279 / 10.941 | 11.681 / 11.477 |

Watch temperature was 38.9-39.4 C; Huawei stayed at 33.0 C. Separating worker
state by 128 bytes did not improve the result. Removing metadata reconstruction
was an intentionally incorrect ablation, and still took 7.840 ms on the watch.
Borrowing tables once per batch instead of checking ownership per shape took
8.552 / 8.621 ms versus interleaved watch baselines 7.556 / 7.495 ms.

A timing probe observed roughly 3.1 ms per watch worker for one third of the
records, about 4-5 ms for the pool on its faster sampled frames, and another
1.5 ms to merge metadata. Queueing and scheduling consume the prototype's gain.
These are CPU diagnostics, not frame-rate acceptance measurements. Raw reports
are in `/tmp/cranpose-mobile-watch-60fps/*-record-scope-*.json`; their `chunks=0`
output field was an unused placeholder, not a measured chunk count.

Correctness evidence for the rejected implementation: all 217 graphics unit
tests and integrations passed; reversing worker completion order failed the
unit test and the public drawing test at record zero. Writing each chunk to GPU
offset zero failed the 5,003-input first frame. Restoring both passed exact
pixel comparisons across rotation, recolouring, changing invalid-arc positions,
and command sizes 5,003 / 10,007 / 4,201 / 123 / 5,017.

Both GPU and capture robot suites completed successfully at `78b24d87`: 162
and four tests respectively, with no skipped tests.


### Admission gate checkpoint and main comparison

`3f948657` incorporates the renderer admission backoff and keeps the key of a
cache hit in its consecutive-frame history. A regression test first reproduces
the sequence admit A, observe B twice, hit retained A, miss B: the original gate
admits B after only one consecutive frame. Observing the hit's actual key before
crediting it restores the required consecutive run and passes the test.

At this checkpoint, full workspace tests and Clippy pass on Linux. Release web,
Android and binary budgets pass. Documentation, robot Clippy, iOS Clippy, device
and simulator build recipes pass on macOS. All 162 GPU robots and four capture
robots pass with zero skipped tests. The isolated-demo binary is 10,426,072 bytes.

The unchanged Showcase app was built twice for Huawei ARM64, against main
`0d195313` and this checkpoint. Full-minute launch-scene measurements, with stable
PID and foreground, present-thread enabled, and battery temperature 34 C:

| Cranpose source | SurfaceFlinger FPS | Frames | Seconds |
| --- | ---: | ---: | ---: |
| Main `0d195313` | 23.6164 | 1,430 | 60.5511 |
| Checkpoint `3f948657` | 23.4819 | 1,423 | 60.6000 |

This comparison demonstrates no launch-scene speed gain. Text, layout and effect
placement look consistent in the captured images; animation makes these captures
unsuitable for a byte comparison. The vivid refracted RGB star streaks are also
present on main. Renderer regression tests provide the deterministic comparisons.

The phone's Mali G76 lacks timestamp-query support. Shader-removal diagnostics
with an ARMv7 test APK gave about 35 FPS during list swipes, about 54 with glass
draws removed, and about 41 with page operations removed. Whole-frame fence
medians were about 50, 34 and 40 ms respectively. These diagnostic ablations
deliberately omit rendering and are not candidates or acceptance measurements;
fences also remove inter-frame overlap. Both runs restored the previous APK and
debug properties. The remaining cost is predominantly GPU shading in this scene.


### Owned recording and shared publication

Removing only the per-append shared-ownership check in an external diagnostic
reduces 15,161-arc public recording from about 7.56 to 7.04 ms on the watch and
1.86 to 1.70 ms on Huawei. The diagnostic snapshots by copying, so it is not the
implementation. It isolates the cost before changing the ownership design.

`CommandRecorder` owns its `ShapeRecorder` while drawing. Finishing moves that
shape recorder into one shared allocation in `CommandRecording`. The GPU retains
that owner and reads its tables directly. Reusing a sole-owned recording keeps
its buffers; a recording with retained readers provides fresh capacity without
changing their data. Explicitly editing a published command makes a copy only
when another reader retains it. Drawing no longer checks an Arc on every shape.

The complete public scope benchmark includes finish and reuse, with changing
angles and all 15,161 records. A second variant retains two published snapshots
to exercise shared-reader reuse. Each entry below is p50 milliseconds from
interleaved controls and candidates; all complete-record fingerprints match
`817059c723455dd8`.

| Device and ownership | Control | Owned recording |
| --- | --- | --- |
| Huawei, sole owner | 1.852 / 1.854 | 1.686 / 1.702 |
| Huawei, retained readers | 2.909 / 2.906 | 2.382 / 2.737 |
| Watch, sole owner | 7.609 / 7.582 | 7.019 / 7.033 |
| Watch, retained readers | 10.704 / 10.562 | 9.811 / 10.029 |

Huawei stays at 33 C; the watch moves from 40.9 to 40.6 C with the apps stopped.
These CPU diagnostics do not establish app frame rate. The publication test
checks that buffer pointers move without a copy and survive unique reuse.
Removing the reuse clear makes the content-reset regression fail, then restoring
it passes. All 216 graphics unit tests and the large mixed-lane/bit-preservation
integrations pass. The seven record goldens, two rotating-run tests and stored
upload pixel test pass on Metal with their original expectations.

### Integrated Megaboss acceptance and removal experiment

Both branches reached `77deb8fe`, with the `f4b83bbf` application runtime.
Full workspace tests, Clippy, release web, Android, binary budgets, macOS
documentation, robot Clippy and iOS recipes pass. All 162 GPU robots and four
capture robots pass with zero skips. The isolated-demo binary is 10,427,608
bytes. These gates did not detect the following device performance regression.

The watch comparison uses the same Cranorbit `0334e16` source, release features,
APK shell and ARMv7-only native packaging. Each leg warms for eight seconds,
then counts actual presents for a full minute. PID and foreground remain stable;
diagnostic overrides are cleared. Runs alternate without a cooling wait.

| Leg | Cranpose | Display FPS | Battery C, start to end |
| --- | --- | ---: | --- |
| 1 | Main `0d195313` | 48.38 | 36.2 to 38.6 |
| 2 | Shared runtime | 42.12 | 39.6 to 41.5 |
| 3 | Main | 28.15 | 41.8 to 42.8 |
| 4 | Shared runtime | 19.50 | 42.9 to 43.4 |
| 5 | Shared runtime | 16.22 | 43.3 to 43.5 |
| 6 | Main | 18.34 | 43.4 to 43.9 |
| 7 | Shared runtime | 16.11 | 43.9 to 44.5 |
| 8 | Main | 18.34 | 44.3 to 45.0 |

The reverse order confirms an approximately 12% regression against main in
the hot workload. This checkpoint fails acceptance. Main logs thousands of
segment-surface composites; the shared renderer retains GPU records but does
not cache those rendered segments. A same-APK removal experiment toggles only
main's existing `debug.cranpose.segment_surface` property. Every leg's log
confirms the property reached the process. This is cause attribution, not a
shipping configuration or a picture acceptance result.

| Leg | Main segment cache | Display FPS | Battery C, start to end |
| --- | --- | ---: | --- |
| 1 | Enabled | 47.84 | 35.9 to 38.9 |
| 2 | Disabled | 44.46 | 39.4 to 41.0 |
| 3 | Enabled | 36.68 | 41.4 to 42.4 |
| 4 | Disabled | 22.54 | 42.5 to 42.8 |
| 5 | Disabled | 22.57 | 42.8 to 43.2 |
| 6 | Enabled | 18.81 | 43.3 to 43.9 |
| 7 | Disabled | 15.74 | 43.9 to 44.3 |
| 8 | Enabled | 17.95 | 44.2 to 44.7 |

At the hottest pair, disabling the cache brings main close to the shared
runtime's result, and re-enabling it restores throughput despite the slightly
hotter leg. This supports the missing reuse as a cause. It does not justify
copying the cache: main resamples rotations, and its rotation test permits a
worst channel difference of 200 across up to 110,000 differing bytes. Exact
static reuse and faster exact drawing remain the permitted design choices.

Raw reports, temperatures, telemetry and screenshots are in
`/tmp/cranpose-mobile-watch-60fps/watch-megaboss-union-*` and
`watch-megaboss-cache-ablation-*`. The app continues to render every required
shape and effect in the accepted implementation; no approximate cache or
reduced workload has been introduced to recover the number.

### Exact body interning feasibility

The captured 15,161 arcs have only 325 distinct 64-byte bodies. An external
probe uses their first 15,007 arcs, all in the same strip class, with 296 unique
bodies. Exact byte keys reduce the body stream from 960,448 bytes to 78,972
including per-instance indices. Curves, draw order, coverage and hardware
blending remain unchanged. Both two-segment and one-segment controls compare
against candidates with the same geometry; no strip reduction is credited to
interning.

On Adreno 702, the one-segment A B A B then B A B A sequence gives packed-body
control medians 35.10 / 35.27 / 35.64 / 35.38 ms and interned medians
36.52 / 36.04 / 36.17 / 35.87 ms. These are submission-to-fence diagnostics,
not app frame times. Battery temperature moves from 43.4 to 43.2 C. All 665,856
output bytes match. Two-segment interning also loses. Reading the shared body
in the fragment stage to reduce varyings is worse: approximately 46–49 ms
against interleaved 23–26 ms controls after the device's throttle state changes.
The hot ARMv7 standard-library hash interning costs another 16–23 ms per table.

Changing every template index to the next body makes the exact comparison fail
in 328,719 bytes, with worst channel difference 171. Restoring the indices
passes. The representation is rejected on performance and has no production
implementation. Source and reports remain in
`/tmp/cranpose-mobile-watch-60fps/template-probe`.


### Full-list routes and Huawei Showcase acceptance

The Huawei route is two forward then two reverse flings at 1.5-second cadence,
(300,1500) to (300,600), 300 ms, native 1080x2244 at density 480. Semantic
checkpoints verify all fourteen bodies and the return to the header. Both main
and the shared runtime reach the same first/last endpoints. Each leg preflights
the endpoints, then measures sixty seconds containing forty gestures. No
accessibility queries or video recording occur in that timed interval. PID,
foreground and SurfaceFlinger layer identity remain stable in all eight legs.

| Leg | Cranpose | Display FPS | Battery C, start to end |
| --- | --- | ---: | --- |
| 1 | Main `0d195313` | 24.99 | 33.0 to 33.0 |
| 2 | Shared runtime `f4b83bbf` | 34.55 | 33.0 to 34.0 |
| 3 | Main `0d195313` | 25.08 | 34.0 to 34.0 |
| 4 | Shared runtime `f4b83bbf` | 34.25 | 34.0 to 35.0 |
| 5 | Shared runtime `f4b83bbf` | 34.82 | 35.0 to 35.0 |
| 6 | Main `0d195313` | 25.27 | 35.0 to 35.0 |
| 7 | Shared runtime `f4b83bbf` | 34.58 | 35.0 to 36.0 |
| 8 | Main `0d195313` | 25.09 | 36.0 to 36.0 |

The shared runtime consistently improves this full route from about 25 to
34–35 FPS, without reaching 60. Endpoint screenshots show consistent layout
and effects; animated content prevents an exact comparison of those device
captures. Existing deterministic GPU expectations remain unchanged.

The watch requires a different verified route: twelve forward then twelve
reverse flings, (100,236) to (100,76), **50 ms**, at 1.5-second cadence. Main
and the shared runtime both reach Proxima and return to the header. Twelve
bodies have visible accessibility labels at checkpoints. Sun and Moon pass
between stops and are confirmed by continuous video; the video is not FPS
acceptance. The earlier 300/500 ms route covered only the initial cards and
must not be described as full-list acceptance. Device resolution remains
408x408 at density 320. The first full-minute matrix finished, but hot shared-
runtime legs 5 and 7 returned only to the first card, rather than the header.
Their counters are process-valid but the matrix is not accepted as a matched
full-scroll comparison. The strengthened route has sixteen steps each way
and an untimed screenshot text assertion before measurement; its repeat is pending.

Protocols, reports and route evidence are under
`/tmp/cranpose-mobile-watch-60fps`: `huawei-full-scroll-route.json`,
`huawei-showcase-full-scroll-union-matrix.json`, `watch-full-scroll-route.json`,
`watch-main-route-endpoints`, `watch-f4-route-middle`, `watch-full-route.mp4`,
`watch-route-opening.png` and `watch-route-earth-moon.png`.

### Arc vertex-kind feasibility

A second external probe keeps the arc-specialized fragment pipeline on both
sides and folds the vertex stage's record kind to arc only on the candidate.
The same 15,007 captured arcs preserve all 665,856 output bytes. The first
A B A B medians are 35.80 / 35.63 / 37.00 / 36.23 ms at 35.4 C. Later legs
move between 15 and 25 ms despite unchanged battery temperature, showing a
device clock transition. There is no stable millisecond-sized saving to
justify a production implementation. The prototype is rejected; source and
raw output are in `/tmp/cranpose-mobile-watch-60fps/vertex-kind-probe`.

### Startup scene validation

The first watch span comparison stopped at its sixth leg because the unchanged
game had paused. The process remained in the foreground, with no application
presents. Its log records a 1,058.1 ms rounded-rectangle pipeline compilation;
the app pauses after a one-second frame-effect gap. Relaunching the same APK
reduces that compilation to 12.4 ms and retains active gameplay. The repeated
comparison checks an untimed scene screenshot before accepting a measurement.
The independent KeepScreenOn wrong-thread exception is being tested against
the actual Android window; it is not treated as proof of the pause's cause.


## Full-minute bounded-span comparison

The APKs in these two matrices contain the bounded-span experiment before the
linear uniform-chunk cursor repair. They keep both applications and all release
features unchanged. Every leg has a stable PID, an awake foreground app and a
live Megaboss preflight; no cooling interval separates the legs.

### Shared runtime versus spans

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | f4 | 52.199 | 35.5 → 38.7 |
| 2 | spans | 50.724 | 39.3 → 41.0 |
| 3 | f4 | 34.065 | 41.2 → 42.1 |
| 4 | spans | 27.909 | 42.0 → 42.7 |
| 5 | spans | 27.943 | 42.5 → 43.1 |
| 6 | f4 | 20.364 | 42.9 → 43.2 |
| 7 | spans | 18.209 | 43.1 → 43.1 |
| 8 | f4 | 16.361 | 42.9 → 43.1 |

### Main versus spans

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | main | 29.218 | 42.2 → 42.9 |
| 2 | spans | 19.659 | 42.9 → 43.5 |
| 3 | main | 18.489 | 43.7 → 43.7 |
| 4 | spans | 18.232 | 43.5 → 43.9 |
| 5 | spans | 18.193 | 43.7 → 43.9 |
| 6 | main | 18.791 | 43.7 → 44.2 |
| 7 | spans | 18.271 | 44.0 → 44.2 |
| 8 | main | 18.572 | 44.0 → 44.2 |

The final shared-runtime pair improves from 16.361 to 18.209 FPS, about 11%.
The subsequent main comparison still fails acceptance: its hot span legs run
18.193–18.271 FPS against main's 18.489–18.791. The remaining 1.4–3.2% loss
keeps the span experiment out of the shared publication branch. Earlier pairs
cross thermal frequency steps and are not interchangeable with these hot pairs.

Review also found that uniform-buffer chunk continuation restarted the prepared
span iterator from the beginning. A persistent cursor consumes each span once
across 128-record chunks. Its index/order test fails when cursor advancement
forgets to move the record start, then passes when restored. Both stored and
uniform GPU parity tests pass on the corrected source. The APK matrices above
do not establish performance of that later source revision.

## Smaller GPU curve feasibility

An external probe removes the two unused normalized-angle floats from GPU
curve attributes, preserving the CPU's original arguments and every operation
the fragment reads. Both sides use the same fixed-arc pipeline and captured
15,007 records. The watch compares all 665,856 bytes exactly. At 41.9 C,
A B A B takes 16.105/15.888/15.850/15.684 ms; B A B A takes
16.569/25.978/16.400/16.459 ms. The first pairs save only 0.17–0.22 ms; the
reverse sequence has one baseline frequency spike and no stable meaningful
saving. The 24-byte curve layout is not adopted.

## Android window-thread correctness

The unchanged Android host's KeepScreenOn call throws
CalledFromWrongThreadException when invoked from the native activity thread.
The AndroidX test captures the actual activity, calls the API directly on a
worker, propagates its exception through a bounded future, waits for the UI
queue, and asserts enable and disable independently. The unmodified method
fails on Huawei and Pixel Watch; dispatching the window mutation through
runOnUiThread passes on both. The isolated test application preserves any
installed demo signed with a different key. This fixes the window call; it
does not remove the separately observed one-second pipeline compilation stall
or establish an FPS improvement. Proofs are retained in the measurement
artifact directory's keep-screen-on/{huawei,watch}-proof.json.


## Verified sixteen-step watch full scroll

All eight legs pass the untimed OCR start check, keep stable foreground/PID,
and complete forty gestures during each full minute. The final-body endpoint
and return screenshots are retained at native 408×408, density 320. The
sequence is main/shared/main/shared, then shared/main/shared/main with no
cooling wait.

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | main | 21.330 | 41.3 → 41.3 |
| 2 | f4 | 29.048 | 41.5 → 41.5 |
| 3 | main | 20.981 | 41.7 → 41.7 |
| 4 | f4 | 29.329 | 41.9 → 41.9 |
| 5 | f4 | 29.055 | 42.1 → 42.1 |
| 6 | main | 21.212 | 42.2 → 42.2 |
| 7 | f4 | 20.593 | 42.3 → 41.9 |
| 8 | main | 18.648 | 41.7 → 41.5 |

The shared runtime leads every adjacent pair. The final two legs cross another
frequency reduction; their lower readings remain in the evidence. This closes
the route gap in the earlier twelve-step matrix, but does not meet 60 FPS.
These APKs precede the effect-domain unit and the Android window-thread fix.

## Precomputed arc raster basis is rejected

An external probe prepares midpoint and padded-half-sweep trigonometry in the
same 32-byte GPU curve column. Only conservative strip geometry consumes that
basis; fragment inputs, draw order and byte counts stay unchanged. All 665,856
bytes match the original shader in the captured 15,007-arc scene. At 39.8 C,
A B A B takes 12.791/12.875/18.566/11.564 ms; B A B A takes
11.683/11.308/27.588/12.733 ms. Both sides encounter frequency excursions,
and the stable pairs show no gain. Preparation adds 3.109 ms on ARMv7.
No production change is justified.


## Cache admission correctness and clean profiling

The full Linux test run has one failure in
`glass_layer_cache::a_cached_glass_result_follows_a_change_beneath_it`: the
changed frame and immediately settled frame differ by a maximum of one
channel level. Removing effect domains leaves the failure unchanged.
Temporarily bypassing admission gives exact zero. A thinner removal also gives
zero while preserving gates, cache insertion and atlas stages: only the
admission frame's replacement of its original composite by `backdrop_blit` is
skipped. Temporary sources were restored and verified after both probes.
The production fix retains source texels and their original sampling description;
it must preserve warm reuse, exact changes and bounded shared-allocation lifetime.

The 939c5ddd + span cursor + Android window-fix APK was profiled with and without
fill-area statistics. Those statistics add approximately 4.4 ms of ARMv7
sin/cos work, inflating median run upload from about 2.01 to 6.44 ms. With them
off, median UI update is 21.28 ms, framework frame work 15.60 ms, graph encoding
7.47 ms, and GPU windows about 23.6–24.1 ms. The instrumented display rate is
not an acceptance number. The clean CPU sample attributes 11.90% self cycles
to arc preparation, 10.00% to shape append, 4.23% to normalization and 3.06% to
sin/cos. Exact instruction sampling also shows record copies and register
spills, so changing function annotations alone is not a design.

A diagnostic APK discards selected shape fragments while retaining recorded
vertices, uploads and draw order. Four 30-second baseline/all-fragment-removal
legs measure displayed FPS 52.79/54.92/42.56/30.87 and GPU medians
18.53/11.03/24.02/22.69 ms at 36.9→38.2/38.8→39.7/40.4→41.7/41.8→42.4 C.
The fifth leg pauses before timing after a newly reached stroke pipeline takes
998.4 ms to compile. It stays invalid; there is no complete reverse-order
comparison. The original APKs and all temporary remote sources are retained
or restored, and diagnostic properties are absent from acceptance runs.


## Exact sweep reuse and pipeline readiness candidates

The sweep cache keeps the same `sin_cos` output under the exact float-bit key.
Eight half-sweep entries cover 99.17% of 15,145 captured non-full-circle arcs;
one entry covers 45.44%. Mid-angle retention stays at one entry.
The complete public drawing/finish/reuse probe, 1,200 frames per leg after warmup,
preserves record fingerprint `817059c723455dd8` in every arm. This CPU probe
runs ARMv7 on both devices; the Huawei release application uses ARM64, so its
release APK comparison is the relevant phone acceptance measurement.

| Device / order A B A B B A B A | Per-frame milliseconds | Temperature |
| --- | --- | --- |
| Huawei, one/eight half-sweep entries | 1.767 / 1.755 / 1.777 / 1.751 / 1.762 / 1.770 / 1.765 / 1.781 | 38 C |
| Watch, one/eight half-sweep entries | 7.020 / 6.992 / 7.057 / 6.807 / 6.772 / 7.054 / 6.783 / 7.190 | 40.1→39.7 C |

Four retained mid-angle entries add no useful improvement. The chosen candidate
has interleaved sweep/eviction and unusual-float correctness tests. Returning
an unrelated cached slot deliberately makes the bit comparison fail; restoring
the keyed lookup passes. These are CPU diagnostics, not app FPS.

A separate same-device Vulkan probe renders during four uncached shape-pipeline
compilations of 552.162 / 557.675 / 580.319 / 568.376 ms. There are 234 / 276 / 273 / 254
completed render/fence cycles during those intervals; worst active frame times
are 9.010 / 24.831 / 18.692 / 6.618 ms. The four control maxima are 7.165 / 4.927 /
6.467 / 19.911 ms, at 37.7→37.5 C. The driver does not globally block drawing for
the compile duration; this is feasibility evidence only.

The candidate uses one bounded worker on Vulkan. Common general pipelines are
ready before active rendering. A required blend/table family is always prepared
before its draw; an optional specialization can finish later. Only a frame
boundary publishes completion, and renderer shutdown cancels queued work.
The existing synchronous behavior remains on other backends. New frame counters
prove that transition tests draw with fallback and later with specialization.
Both storage and uniform-table tests pass with zero differing bytes on Linux
Vulkan and Pixel Watch, across clipping, overlap, SrcOver/DstOut/Plus, stored
and transient runs, and changed records. Deliberately substituting SrcOver for
the required fallback blend makes 34,364 bytes differ; restored tests pass.
The existing specialized/general tests also wait for actual readiness and
report zero differences on the watch in all three scenes. Driver capability
warnings on the watch are retained in the logs.

Shape spans are preserved in a stash and excluded from the independent release
candidate. Both comparison libraries use Rust 1.98.0 and unchanged Cranorbit `0334e16`
benchmark inputs; only framework libraries differ. The four default comparisons
against shared `071a6e9c` are recorded below; combined acceptance against main
remains required.


## Independent pipeline-readiness and eight-sweep candidate

The control is shared `071a6e9c`; the candidate changes only Cranpose's
optional Vulkan pipeline preparation and exact half-sweep cache. Both arms
use the same updated Android host per application and unchanged application
source. Huawei APKs contain ARM64; watch APKs contain ARMv7. The CPU probe
reported above uses ARMv7 on both devices and is a separate diagnostic.
Every matrix runs A B A B followed by B A B A, sixty seconds per leg without
a cooling wait. All reported legs keep stable process/foreground and pass
workload checks; Showcase traverses the full route with forty gestures.

### Huawei Megaboss

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | shared071 | 59.910 | 40.0 → 41.0 |
| 2 | readiness | 59.890 | 42.0 → 41.0 |
| 3 | shared071 | 59.872 | 41.0 → 42.0 |
| 4 | readiness | 59.905 | 42.0 → 42.0 |
| 5 | readiness | 59.884 | 41.0 → 42.0 |
| 6 | shared071 | 59.864 | 42.0 → 43.0 |
| 7 | readiness | 59.863 | 43.0 → 43.0 |
| 8 | shared071 | 59.880 | 43.0 → 43.0 |

### Watch Megaboss

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | shared071 | 52.087 | 37.3 → 39.3 |
| 2 | readiness | 51.223 | 39.9 → 41.5 |
| 3 | shared071 | 37.271 | 41.5 → 42.3 |
| 4 | readiness | 25.568 | 42.2 → 42.6 |
| 5 | readiness | 25.084 | 42.3 → 42.5 |
| 6 | shared071 | 25.118 | 42.5 → 42.6 |
| 7 | readiness | 25.100 | 42.5 → 42.7 |
| 8 | shared071 | 25.116 | 42.5 → 42.7 |

### Huawei Showcase full scroll

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | shared071 | 39.380 | 42.0 → 43.0 |
| 2 | readiness | 39.339 | 43.0 → 44.0 |
| 3 | shared071 | 38.951 | 44.0 → 43.0 |
| 4 | readiness | 39.044 | 43.0 → 43.0 |
| 5 | readiness | 39.197 | 45.0 → 45.0 |
| 6 | shared071 | 36.894 | 44.0 → 44.0 |
| 7 | readiness | 39.020 | 44.0 → 46.0 |
| 8 | shared071 | 39.175 | 44.0 → 46.0 |

### Watch Showcase full scroll

| Leg | Build | Displayed FPS | Battery C, start → end |
| --- | --- | ---: | --- |
| 1 | shared071 | 41.107 | 39.3 → 40.4 |
| 2 | readiness | 29.710 | 41.1 → 41.3 |
| 3 | shared071 | 29.840 | 41.4 → 41.5 |
| 4 | readiness | 29.681 | 41.5 → 41.6 |
| 5 | readiness | 29.615 | 41.6 → 41.7 |
| 6 | shared071 | 29.632 | 41.7 → 41.7 |
| 7 | readiness | 29.760 | 41.8 → 41.8 |
| 8 | shared071 | 29.765 | 41.9 → 41.9 |

Huawei Megaboss stays at 59.86–59.91 FPS. The watch's hot reverse-order
pairs differ by −0.035 and −0.016 FPS near 25.1 FPS, effectively flat. Its
large negative second pair, 37.27 versus 25.57 FPS across the thermal step,
remains in the table; the experiment does not establish a sustained speedup.
Huawei Showcase pairs differ by −0.041, +0.094, +2.303 and −0.155 FPS.
Three pairs are effectively flat; one control leg slows to 36.89 FPS.
These results neither establish 60 FPS nor replace the required combined
comparison against main. Watch full scroll crosses the thermal step in its
first pair, 41.107 versus 29.710 FPS. The next three B-minus-A pairs are
−0.159, −0.017 and −0.005 FPS; the hot reverse pairs are effectively flat.
The native SurfaceFlinger histogram labels put hot watch p50/p95/p99 periods
in the same 33/50/50 ms buckets for both workloads and both builds. Huawei
Megaboss remains in 16/16/16 ms buckets, and its full-scroll p95/p99 stay at
33/50 ms. These are histogram bucket labels, not exact percentile durations.
The final watch candidate scroll leg has one 102 ms bucket sample; the last
control leg tops out at 66 ms. Individual tails remain recorded alongside
the averages; the candidate is not described as a universal latency win.

Raw reports and APK provenance are in `/tmp/cranpose-mobile-watch-60fps`,
with matrix labels `huawei-shared071-readiness`, `watch-shared071-readiness`
and `{huawei,watch}-showcase-shared071-readiness`. Histogram summaries are in
`readiness-frame-period-histograms.json`. The `orbit-*-provenance.json`
and `cranpose-showcase-*-provenance.json` files record host, native library,
source and packaged APK hashes.

### Exact band-template cache rejected

An external sixteen-entry cache keys normalized inner/outer radii and sweep
bits, retaining band padding and strip class. It hits 98.18% of captured
keys but increases complete drawing/finish/reuse cost on the watch. A B A B
then B A B A gives 7.007 / 10.148 / 6.985 / 10.157 / 10.210 / 7.024 /
10.196 / 7.000 ms, with every fingerprint `817059c723455dd8`. Battery
temperature moves from 39.8 to 38.4 C during the first leg and stays at
38.4 C for all later starts and ends. The candidate is roughly 45% slower;
no production change is retained. The source restoration was verified.


## Recording storage eligibility follows both owners

A diagnostic APK adds counters only to large-command storage acquisition.
The current two-slot pool takes a sole outer Rc while the inner shape Arc is
still retained by the present packet and stored run. Its samples show one slot
with outer count 1, inner count 3 and capacity 32,768, followed by an empty
spare. Of 1,408 acquisitions, 1,405 take shared inner columns and only three
reuse storage. Taking that newest slot means publication rotates None into
the spare, defeating the pool.

A temporary eligibility change requires both owner counts to be one. It
restores an older free slot alongside the newest shared slot and reuses
storage in 1,900 of 1,920 acquisitions. No extra slot, frame queue depth,
drawing work or quality setting changes. The second diagnostic's raw
no_outer label means no candidate satisfying both checks; its twenty misses
must not be read as twenty outer-Rc conflicts. Diagnostic FPS is not
acceptance. Both temporary instrumented sources were restored after building.

The production predicate has two focused scene-builder tests for retained
inner/outer readers, older-slot preservation, changing counts and colours,
and clearing content markers. Both fail on the original predicate and pass
with the ownership check. Deliberately omitting the reuse clear makes one
new shape become five and prevents an empty frame from being empty; both
tests fail, then pass after restoration. The tests also compare the retained
frame's original records while later frames are written.

Evidence is under /tmp/cranpose-mobile-watch-60fps in
watch-readiness-reuse-owners and watch-readiness-reuse-eligible-owners.
Linux RED, GREEN, stale-content and restored logs are
/tmp/cranpose-recording-storage-*.log on samarch-1. Default release libraries
without counters are being built for both applications and ABIs; their
performance is still unmeasured.

## Two-owner recording storage: separate Megaboss acceptance (2026-09-06)

A is shared071 plus pipeline readiness and the exact eight-sweep cache. B adds
only the recording pool eligibility fix. Both are default release native builds
with the same Java hosts, no diagnostic counters, unchanged applications and
native display geometry. All sixteen full-minute legs have valid scene and
process checks. ABAB then BABA runs without cooling waits.

### Huawei Megaboss

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 59.910026 | 38.0 → 39.0 |
| 2 | B | 59.897855 | 39.0 → 40.0 |
| 3 | A | 59.893043 | 39.0 → 40.0 |
| 4 | B | 59.917448 | 40.0 → 41.0 |
| 5 | B | 59.885512 | 41.0 → 41.0 |
| 6 | A | 59.893480 | 42.0 → 43.0 |
| 7 | B | 59.909583 | 43.0 → 42.0 |
| 8 | A | 59.909575 | 42.0 → 42.0 |

Adjacent B-minus-A pairs: -0.012172, +0.024406, -0.007968, +0.000008 FPS.

### Watch Megaboss

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 52.187695 | 34.1 → 37.1 |
| 2 | B | 52.136312 | 37.6 → 39.6 |
| 3 | A | 50.898319 | 39.8 → 41.6 |
| 4 | B | 37.264440 | 41.8 → 42.3 |
| 5 | B | 25.752940 | 42.2 → 42.6 |
| 6 | A | 25.055020 | 42.4 → 42.7 |
| 7 | B | 25.091822 | 42.5 → 42.8 |
| 8 | A | 25.074426 | 42.6 → 42.9 |

Adjacent B-minus-A pairs: -0.051383, -13.633879, +0.697919, +0.017396 FPS.

Huawei remains display-limited near 59.9 FPS. The watch crosses a thermal
step in the second pair; that negative result stays in the record. Its hot
reverse pairs are +0.698 and +0.017 FPS, the final pair effectively flat near
25.1 FPS. Restored column reuse is proven, but this unit does not resolve the
GPU deadline gap. Full-scroll comparisons are still running.

The first combined641 Showcase artifact was rejected before measurement: it
was identical to the preceding native library despite changed framework source.
Source archives had preserved timestamps older than the previous build. After
hash verification and refreshing the full source inventory timestamps, all
relevant framework crates recompiled. The new builder records resolved framework
manifest paths and hashes every app workspace member, then verifies packaged
native bytes. The independent readiness and ownership builds show compilation
of the relevant graphics/common/wgpu crates in both ABIs. One shared071 Huawei
Showcase control used a warm build; a fresh rebuild is being compared with its
retained library before relying on that earlier control.

The fresh shared071 reproduction matches both Megaboss libraries and watch
Showcase byte-for-byte. Huawei Showcase differs: measured `203e2d81`, rebuilt
`ddec3d14`. Therefore `huawei-showcase-shared071-readiness-matrix.json` is
withdrawn as evidence of the readiness candidate effect. Its route, temperatures
and observed FPS remain recorded, but the reference source was wrong. The
correct control must be measured before making that comparison again. The
other three shared071/readiness controls are reproduced exactly.

### Huawei full-scroll rejection of two-owner recording reuse

All eight legs have valid native-resolution full-route checks and forty timed
swipes. A is readiness; B adds only two-owner storage eligibility.

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 39.471795 | 41.0 → 42.0 |
| 2 | B | 39.178525 | 42.0 → 43.0 |
| 3 | A | 39.477580 | 42.0 → 43.0 |
| 4 | B | 38.726398 | 43.0 → 43.0 |
| 5 | B | 36.230533 | 43.0 → 44.0 |
| 6 | A | 39.049713 | 43.0 → 44.0 |
| 7 | B | 35.743322 | 44.0 → 44.0 |
| 8 | A | 38.523401 | 44.0 → 45.0 |

B-minus-A is -0.293, -0.751, -2.819 and -2.780 FPS. Both hot reverse pairs
lose about seven percent, even though the control finishes as warm or warmer.
The ownership unit and its tests are stashed outside the active branch. Its
correct storage recycling does not justify this measured regression. The
combined641 APKs containing it are also held, not adoption candidates.

### Watch full-scroll completion of the held ownership experiment

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 31.844827 | 40.7 → 41.3 |
| 2 | B | 29.733867 | 41.6 → 41.7 |
| 3 | A | 29.602022 | 41.9 → 42.0 |
| 4 | B | 29.597488 | 42.2 → 42.2 |
| 5 | B | 20.911768 | 42.2 → 41.9 |
| 6 | A | 20.994889 | 41.9 → 41.7 |
| 7 | B | 27.183146 | 41.6 → 41.5 |
| 8 | A | 29.639471 | 41.7 → 41.8 |

All eight legs pass full-route checks with forty timed swipes. B-minus-A
pairs are -2.111, -0.005, -0.083 and -2.456 FPS. Both arms fall near21FPS in
the middle hot reverse legs; the final B leg remains below its neighboring
control. No frame or temperature is discarded. The unit stays held. Fresh
audited rebuilds reproduce both readiness and readiness-reuse Showcase native
libraries byte-for-byte in both ABIs, confirming these paired builds.

## Audited controls and fresh-main comparisons, 2026-09-06

All legs below use full-minute SurfaceFlinger measurements, unchanged apps, native resolution and density, default renderer settings and common Java hosts. Source inventories and native hashes were audited after refreshing archive timestamps. Every leg is retained; the order is ABAB then BABA without cooling waits.


### Corrected Huawei Showcase control

A is freshly reproduced shared071; B is independent pipeline readiness plus exact eight-entry sweep reuse. All eight legs pass full-route checks with forty timed swipes. This replaces the withdrawn stale-control comparison; it does not rehabilitate that earlier build.

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 38.186140 | 39.0 → 40.0 |
| 2 | B | 38.972758 | 41.0 → 41.0 |
| 3 | A | 38.881507 | 41.0 → 42.0 |
| 4 | B | 39.762986 | 40.0 → 41.0 |
| 5 | B | 39.421926 | 41.0 → 41.0 |
| 6 | A | 37.174794 | 42.0 → 42.0 |
| 7 | B | 39.313346 | 43.0 → 44.0 |
| 8 | A | 38.650075 | 42.0 → 44.0 |

Adjacent B-minus-A pairs: +0.786618, +0.881480, +2.247132, +0.663271 FPS.

The corrected comparison favors readiness in all four pairs. The final hot pair is +0.663 FPS at 42–44 C. This supports retaining the independent unit against its shared071 control; the workload remains below 60 FPS.


### Fresh-main watch Megaboss

A is freshly rebuilt main0d195313; B is independent readiness on shared071. All eight scene/process checks pass. The held ownership experiment is absent.

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 49.271667 | 39.1 → 41.2 |
| 2 | B | 37.254602 | 41.3 → 42.0 |
| 3 | A | 29.955132 | 42.0 → 42.6 |
| 4 | B | 25.069592 | 42.8 → 42.9 |
| 5 | B | 25.062695 | 42.8 → 43.1 |
| 6 | A | 28.326505 | 43.0 → 43.4 |
| 7 | B | 16.949807 | 43.3 → 43.4 |
| 8 | A | 18.778722 | 43.0 → 43.2 |

Adjacent B-minus-A pairs: -12.017065, -4.885540, -3.263810, -1.828915 FPS.

Main remains ahead in both hot reverse pairs: 28.326505 versus 25.062695 FPS, and 18.778722 versus 16.949807 FPS. The losses are 11.522% and 9.739%. Earlier negative pairs include thermal crossings and remain recorded. This confirms the shared renderer still fails the no-regression gate against main; PR #617 must not land.

## Fresh-main full-scroll comparisons, 2026-09-06

A is the freshly rebuilt main0d195313; B is independent readiness and eight-sweep reuse on shared071. Both use the same Java hosts and unchanged app source. Every native-resolution leg has sixty seconds, forty timed gestures, full-route endpoint checks and a stable foreground process. ABAB then BABA runs without cooling waits.


### Watch

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 21.231832 | 41.1 → 41.3 |
| 2 | B | 29.656878 | 41.6 → 41.6 |
| 3 | A | 21.231029 | 41.7 → 41.7 |
| 4 | B | 29.763457 | 42.0 → 42.0 |
| 5 | B | 29.677252 | 42.0 → 42.0 |
| 6 | A | 21.030356 | 42.1 → 42.0 |
| 7 | B | 29.790801 | 42.2 → 42.2 |
| 8 | A | 20.828932 | 42.3 → 42.2 |

Adjacent B-minus-A pairs: +8.425046, +8.532428, +8.646896, +8.961869 FPS.


### Huawei

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 28.555566 | 39.0 → 41.0 |
| 2 | B | 39.685197 | 41.0 → 42.0 |
| 3 | A | 29.028877 | 41.0 → 42.0 |
| 4 | B | 39.485239 | 41.0 → 42.0 |
| 5 | B | 39.910314 | 41.0 → 42.0 |
| 6 | A | 27.821496 | 43.0 → 43.0 |
| 7 | B | 36.746924 | 43.0 → 42.0 |
| 8 | A | 26.053806 | 43.0 → 44.0 |

Adjacent B-minus-A pairs: +11.129631, +10.456362, +12.088818, +10.693118 FPS.

Both complete comparisons favor the shared renderer in every pair. They confirm full-scroll improvement against a fresh main reference, while remaining well below 60 FPS. They exclude the later prefix/source/layout units and the held kind-range experiment. Watch Megaboss still loses against main; these gains do not satisfy that separate gate.


## Isolated kind-range correctness, 2026-09-06

The layout528 candidate combines the reviewed 528815a3 stage-layout snapshot,
independent readiness/eight-sweep changes and ordered long-kind spans. It
excludes the held two-owner storage change. Complete source inventories, native
hashes and unchanged Orbit/Showcase app inventories accompany both ABI builds.
It is an isolated experiment, not a committed optimization.

Macm3 proofs pass with fixed code, fail when the window start is left untrimmed
or a span selects the wrong shape kind, and pass after restoration. GPU tests
wait for actual specialization and check every pending frame. The Pixel Watch
also passes two span parity tests (15.72 s), two pipeline-transition tests
(13.73 s) and five opaque-prefix tests (32.75 s). Every binary hash matches its
source provenance; the device is awake and benchmark apps are stopped.
These are correctness measurements, not presented FPS.

The final ee467612 layout snapshot passes 26 Linux atlas/glass tests and the
padding unit. Dropping sampling-layout identity fails the shader probe;
restoration passes and source hashes match. The earlier 528 snapshot with
readiness passes full workspace tests, native/wasm Clippy, release web, and
both CI robot partitions including all four framebuffer capture tests.


## Fresh-main versus isolated kind ranges on watch Megaboss

A is audited main0d195313; B is layout528-kind-spans, with the ownership
experiment absent. Unchanged apps, native resolution, default runtime settings,
sixty seconds per leg, ABAB then BABA with no cooling wait. All eight process
and scene checks pass.

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 49.367088 | 34.9 → 37.4 |
| 2 | B | 53.331658 | 38.0 → 40.2 |
| 3 | A | 42.808855 | 41.0 → 41.9 |
| 4 | B | 40.985991 | 41.8 → 42.6 |
| 5 | B | 27.613193 | 42.8 → 42.9 |
| 6 | A | 28.609143 | 42.7 → 43.1 |
| 7 | B | 27.603141 | 43.0 → 43.2 |
| 8 | A | 26.069040 | 43.0 → 43.3 |

Adjacent B-minus-A pairs: +3.964569, -1.822864, -0.995950, +1.534100 FPS.

The hot candidate is stable at 27.613 and 27.603 FPS; adjacent main is 28.609
and 26.069 FPS. One hot pair loses 3.48% and the other gains 5.88%. The gap is
smaller than the readiness-only comparison, but no-regression acceptance is
not established. All thermal crossings remain in the data. Kind ranges stay
isolated pending further work and full final-source comparisons.


### Current candidate attribution and rejected arc invariant move

A separate 30-second sampling run of layout528-kind-spans, with fill-area
diagnostics off, has median UI update 19.015 ms, scene production 12.055 ms,
framework frame 13.81 ms, run upload 1.85 ms and renderer graph execution
6.89 ms. The 29 GPU windows have median span 17.540 ms
(range 17.010–24.360 ms). These stages overlap and their
percentiles are not added. The diagnostic begins at 39.9 C and is not a
steady-FPS acceptance comparison. Both CPU and GPU still exceed the budget.

An external shader probe moves `(outer + inner) * 0.5` and
`max((outer - inner) * 0.5, 0.0)` from each fragment to the flat vertex output.
Input records, strip geometry, draw order and varying size are unchanged.
At scale 0.75 its 665,856 bytes match; at 1.25, 24 channel bytes differ by one
level on Adreno 702. That rejects the change before timing or production
integration. Identical source arithmetic across shader stages does not
establish identical pixel results. The extended endpoint variant is not
adopted. The probe uses wgpu 29.0.4; the application uses its audited lockfile.


### Scope of the earlier standalone CPU recorder numbers

The early `record-bench` fixtures reconstruct a synthetic arc stream from
captured GPU body/curve columns. Those columns omit the original source
arguments. A later flag census confirms all 15,161 captured entries are
arcs; the source reconstruction still cannot recover the omitted argument bits. Its paired recorder timings remain measurements of that synthetic
workload; they are not a bit-faithful replay of the application or a substitute
for the default app FPS matrices above. The eight-sweep unit has independent
bit-preservation tests and real application comparisons. Further producer
experiments must use the actual ArenaRenderer calls or a capture retaining
every original argument and shape kind.

## Shared-a571 call-boundary removal: no measured gain

A is `kind-a571`; B is the identical source inventory except for
`#[inline(always)]` on private `push_shape`. Both use accepted runtime `a5710463`
plus the isolated kind-range candidate. All 70 application source files remain
unchanged. Native binaries for both ABIs are rebuilt and APK library hashes
verified. The ARMv7 call disappears and the caller's local stack reservation
shrinks from 160 to 144 bytes. Both graphics unit suites pass, as do the final
combined Metal span, variant and prefix tests.

Default watch Megaboss, eight full-minute legs, ABAB then BABA, no cooling:

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 47.622646 | 40.5 → 41.6 |
| 2 | B | 40.917621 | 41.8 → 42.6 |
| 3 | A | 27.656329 | 42.5 → 42.8 |
| 4 | B | 27.574609 | 42.7 → 42.9 |
| 5 | B | 27.603849 | 42.8 → 42.9 |
| 6 | A | 27.629851 | 42.7 → 42.9 |
| 7 | B | 27.632235 | 42.7 → 43.0 |
| 8 | A | 27.600115 | 42.8 → 43.0 |

All process and scene checks pass. Adjacent B-minus-A pairs are -6.705025,
-0.081719, -0.026003 and +0.032120 FPS. The hot pairs are effectively flat;
the early thermal crossing is retained. The experiment is rejected as having
no measured benefit and remains outside the shared runtime.

## Actual producer data census

The unchanged application's public drawing producer emits 16,958–17,527 shape
records per sampled frame. At 20 updates/s, two of 180 post-warm-up recordings
have an entirely unchanged body column; 44.12% of bodies equal the previous
record at the same index. At 60 updates/s, those counts are 51/540 and 46.77%.
Existing 4 KiB comparison already avoids 34.38% and 37.81% of body upload bytes,
respectively, with a median of one merged upload range for each column.
This is a fixed-rate data census, not an application timing or FPS result.
It rejects the premise that most of this workload can share a whole immutable
body column merely because angles animate.

## Final shared-a571 versus fresh main: Huawei Megaboss

A is the audited fresh-main `0d195313` rebuild; B is the pure shared runtime
`a5710463`, with neither held ownership reuse nor kind spans. Both unchanged-app
APKs use the same host and audited native libraries. Default settings, native
resolution, sixty seconds per leg, ABAB then BABA, no cooling wait.

| Leg | Variant | FPS | Temperature C |
| --- | --- | --- | --- |
| 1 | A | 57.075488 | 45.0 → 46.0 |
| 2 | B | 59.811924 | 46.0 → 43.0 |
| 3 | A | 57.656814 | 43.0 → 44.0 |
| 4 | B | 59.833693 | 44.0 → 43.0 |
| 5 | B | 59.854798 | 43.0 → 42.0 |
| 6 | A | 56.529199 | 42.0 → 42.0 |
| 7 | B | 59.874835 | 42.0 → 41.0 |
| 8 | A | 56.485458 | 41.0 → 41.0 |

All scene and process checks pass. B-minus-A pairs are +2.736436, +2.176879,
+3.325599 and +3.389377 FPS. This comparison favors the final shared runtime
in every pair and places Huawei Megaboss near its display limit. It does not
establish acceptance for watch Megaboss or either full-scroll Showcase workload.
