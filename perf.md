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

* [ ] **Profile heap allocations to find top callers**: use `valgrind --tool=massif` or add `#[global_allocator]` counting wrapper to identify which code paths are responsible for the bulk of malloc/cfree. The DWARF profile shows `alloc::raw_vec::finish_grow` → Vec resizing is significant. Identify the top 5 allocation sites by volume.

* [ ] **Reduce per-frame Vec allocations in layout**: `measure_node` (1.16%) and `measure_through_modifier_chain` (0.67%) are the top cranpose functions. `VecPools` already exists but `drop_in_place<VecPools>` at 0.12% and `drop_in_place<LayoutBox>` at 0.12% suggest Vecs are still being allocated/dropped per frame. Audit the layout pass for Vecs that could be pooled or reused across frames via `clear()` + reuse instead of drop + alloc.

* [x] **Pool or arena-allocate modifier chain Vecs**: Made `NodeLink` Copy (since `NodePath` is Copy), eliminated all `.clone()` on NodeLink/NodePath. Replaced heap-allocated `Vec<usize>` path buffer in `rebuild_ordered_nodes` with stack-based `[usize; MAX_DELEGATE_DEPTH]`. `ordered_nodes` Vec already reused via `clear()` + `push()`.

* [x] **Eliminate per-frame `single_fingerprints` rehashing**: Replaced full-chain rehashing in `Modifier::then()` with appendable fingerprint state. `Modifier` now tracks `element_count`, `then()` folds only the newly appended elements into the existing strict/structural fingerprints, and unit tests cover split-append vs full-pass fingerprint equivalence plus `Modifier::from_parts()` parity.

## Render pipeline (1.37% cranpose + 10.6% wgpu)

* [ ] **Reduce wgpu render pass churn**: `drop_in_place<RenderPass>` (0.52%), `drop_in_place<Tracker>` (0.30%), `begin_render_pass` (0.34%), `insert_barriers_from_scope` (0.42%), `set_bind_group` (0.54%) — wgpu overhead totals ~10.6%. `render_non_effect_segment` (0.90%) creates/drops render passes. Audit how many render passes per frame and whether adjacent same-target passes can be merged to reduce begin/end/barrier overhead.

* [ ] **Batch `queue.write_buffer` calls**: `Queue::write_buffer` at 0.48%, `Queue::submit` at 1.17%. Each write_buffer is a separate staging copy. Consider using a single staging buffer with sub-allocations and one write per frame instead of many small writes.

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
