# Performance Work — Phase 2

Baseline: **1620–2410 FPS** (0.41–0.62ms) on `native-release`, headless robot_perf_harness, 10s run.
Profile is flat — no single app function above 1.2%. Allocator at ~6%, WGPU at ~11%, nvidia driver at ~5%.

Cranpose total self-time: **14.2%** across ~25 functions. Below are the actionable items.

## Agent prompt

> You are continuing performance optimization on the cranpose Rust UI framework.
> Read `perf.md` and `AGENTS.md` for context and rules. The profile is flat — there are no single hotspots above 1.2%.
> The work is to systematically reduce the allocator overhead (6% CPU: malloc 2.85%, cfree 2.17%, realloc+finish_grow 1.2%) and the per-frame cranpose overhead (14.2%).
> For each item below, do the actual implementation work (code changes, tests, clippy, fmt), not just analysis.
> After each item, mark it `[x]` in perf.md with a brief description of what changed.
> Run `cargo test`, `cargo clippy --workspace`, `cargo fmt` after each change.
> Follow all rules in AGENTS.md strictly: zero warnings, all tests pass, no shortcuts, no half-states.

---

## Allocator pressure (6% CPU total)

* [x] **Profile heap allocations to find top callers**: Reworked `perf_robot_heap.sh` into a repeatable allocator-profiling entry point with `heaptrack` auto-detect, `massif` fallback, and peak-snapshot top-caller extraction from `ms_print`. Added `CRANPOSE_PERF_TIMEOUT_SLACK_SECS` support to `robot_perf_harness` so allocator profilers do not trip the normal watchdog. A 1s `native-release` `massif` run (`perf_heap_profile_*`) peaked at **29.38MB** total heap, and the top allocation branches by volume were: wgpu command encoder begin-encoding / staging path **8.69MB (29.56%)**, long tail of **1845** sub-1% sites **5.85MB (19.91%)**, wgpu surface configure **4.31MB (14.68%)**, EGL make-current **2.83MB (9.63%)**, and EGL context creation **2.32MB (7.89%)**. `alloc::raw_vec::finish_grow` is still present at **0.89MB (3.02%)** across **108** sites, which confirms the Vec-growth cost is diffuse and the next layout pooling pass should target aggregate reuse rather than a single hotspot.

* [x] **Reduce per-frame Vec allocations in layout**: Replaced the single-spare `VecPools` slots with stack-backed `ScratchVecPool`s so nested `measure_node()` recursion now keeps multiple scratch `Vec<Box<dyn Measurable>>`, child-record, child-id, and modifier-node buffers alive instead of reallocating and then dropping them as outer frames overwrite the one spare slot. `LayoutNodeSnapshot` no longer clones `children: Vec<NodeId>` on every measure; `measure_layout_node()` copies child IDs into a pooled scratch vec instead, and `measure_through_modifier_chain()` reuses pooled modifier-node scratch storage. Subcompose and measured-child output vecs now pre-size from known child counts. Added a nested-measurement regression test that proves multiple scratch vecs survive the pass. A follow-up 1s `native-release` `massif` run (`perf_layout_heap_profile_*`) reduced peak heap from **29.38MB** to **26.99MB**, while the diffuse `alloc::raw_vec::finish_grow` branch moved from **0.89MB (3.02%)** to **0.80MB (2.97%)**.

* [x] **Pool or arena-allocate modifier chain Vecs**: Made `NodeLink` Copy (since `NodePath` is Copy), eliminated all `.clone()` on NodeLink/NodePath. Replaced heap-allocated `Vec<usize>` path buffer in `rebuild_ordered_nodes` with stack-based `[usize; MAX_DELEGATE_DEPTH]`. `ordered_nodes` Vec already reused via `clear()` + `push()`.

* [x] **Eliminate per-frame `single_fingerprints` rehashing**: Replaced full-chain rehashing in `Modifier::then()` with appendable fingerprint state. `Modifier` now tracks `element_count`, `then()` folds only the newly appended elements into the existing strict/structural fingerprints, and unit tests cover split-append vs full-pass fingerprint equivalence plus `Modifier::from_parts()` parity.

## Render pipeline (1.37% cranpose + 10.6% wgpu)

* [x] **Reduce wgpu render pass churn**: Reworked `render_non_effect_segment` around an allocation-free chunk iterator that groups non-conflicting shape/image/text batches into one render pass per encoder chunk. Shapes, images, and text are now prepared before pass recording and then drawn in-order inside a shared `Segment Pass`, while repeated buffer kinds and shadows still force chunk boundaries so GPU buffer rewrites stay correct. The first `LoadOp::Clear` is now consumed only when a chunk actually records work, which also fixes no-op first-batch cases.

* [x] **Batch `queue.write_buffer` calls**: Added a reusable `Frame Upload Buffer` in the native wgpu renderer and staged hot-path uniform/shape/image uploads into one packed CPU batch per encoder chunk. `render_non_effect_segment()` and offscreen shape rendering now do a single `queue.write_buffer()` to the upload buffer, then `copy_buffer_to_buffer()` into the destination GPU buffers before drawing, eliminating the previous 3-6 small `queue.write_buffer()` calls per shape/image batch while preserving the existing flush boundaries for reused batch buffers. The wasm target keeps direct queue writes because the browser backend regressed with the staged copy path.

## Semantics and metadata overhead (1.16%)

* [x] **Skip semantics collection when not needed**: Added `MeasureLayoutOptions { collect_semantics }` plus `measure_layout_with_options()`, kept `measure_layout()` semantics-on by default, and made `AppShell` opt into semantics only when a runtime consumer enables it. The shell now skips semantics tree construction by default, enables it for robot mode, and preserves `needs_semantics` dirtiness while disabled so enabling later rebuilds correctly. Runtime metadata no longer calls `semantics_configuration()` just to discover text roles; it derives text role from cached modifier slices instead.

## Modifier chain overhead (0.56%)

* [x] **Make `NodePath` Copy to eliminate SmallVec clone cost**: `NodeLink::clone` at 0.32% is cloning `SmallVec<[usize; 2]>` inside `NodePath` on each iteration step. `NodePath` has `entry: usize` + `delegates: SmallVec<[usize; 2]>`. If max delegate depth is bounded (which it should be — modifier delegation rarely exceeds 2–3 levels), use `[usize; N]` + `len: u8` to make it Copy. This eliminates the clone cost entirely.

* [x] **Reduce `aggregate_child_capabilities` cost**: Extended ordered_nodes to 3-tuple `(NodeLink, NodeCapabilities, NodeCapabilities)` where third element is pre-computed aggregate. `ModifierChainNodeRef` caches it, avoiding RefCell borrows on hot path.

## Low-priority / future

* [ ] **Tame `Mutex<FontSystem>` for future parallelism**: render + measurement share `Arc<Mutex<FontSystem>>`. Single-thread now, blocks future parallel layout.

* [ ] **Dependency-dup cleanup**: `cargo tree --duplicates` shows duplicates (getrandom, smol_str, tiny-skia, ttf-parser). Not a speed lever.

---

## Completed (phase 1)

<details><summary>27 completed items from phase 1</summary>

* [x] Stop using size-tuned release for native perf — separate `native-release` profile with `opt-level=3` (+46.6%)
* [x] Fix P0 leak: SnapshotStateObserver::fast_scopes grows without bound
* [x] Fix P0 leak: observer scopes accumulates because clear never happens
* [x] Fix P0 leak: StateArena::cells never frees — generation-checked reusable slots
* [x] Fix P0 leak: dead watchers accumulate in MutableStateInner::watchers
* [x] Remove P1 hot-path O(n) work on every state read
* [x] Fix P1 O(n²) child diffing in pop_parent()
* [x] Fix P1 O(n²) scope grouping in process_invalid_scopes()
* [x] Cut allocation pressure: replace Box<dyn FnMut> commands with enum
* [x] Cut recomposition copying: make snapshot_locals COW
* [x] Fix pointer latency: stop cloning full hit-region list on every event
* [x] Stop double-storing hit regions
* [x] Avoid per-batch temporary Vec allocations in renderer
* [x] Stop full scene rebuild for "any dirty bit"
* [x] Skip clean subtrees in layout box refresh
* [x] Text bottleneck: renderer-side per-batch TextRenderer pool with skip optimization
* [x] Unify text identity across measure + render
* [x] Re-architect text around node-local paragraph state
* [x] Replace shared shaped-buffer cache HashMap with LRU
* [x] Fix wrapped text reshaping on fast path
* [x] Zero/low-copy text wrapping rewrite
* [x] Remove SipHash/DefaultHasher from internal hot hashes
* [x] Flatten modifier/layout hot reads — cached capabilities, Vec-first get_mut
* [x] Eliminate ModifierKind::Combined recursive Rc tree
* [x] Eliminate Box<dyn Placeable> allocations in layout
* [x] Replace IndexSet<NodeId> children storage with Vec<NodeId>
* [x] Cap renderer caches and instrument memory growth

</details>
