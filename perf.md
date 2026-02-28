* [ ] **Stop using size-tuned `release` for native perf**: in `Cargo.toml` set up *separate* profiles; current native `release` uses `opt-level = "s"` (evidence: `Cargo.toml:62`). For perf runs/builds use a speed profile (`opt-level=3` etc). Benchmark proof (same harness/env): `CRANPOSE_HEADLESS=1 CRANPOSE_PRESENT_MODE=immediate CRANPOSE_PERF_DURATION_SECS=5` → default `release` **1366.8 FPS (0.73ms)** vs `CARGO_PROFILE_RELEASE_OPT_LEVEL=3` **2003.8 FPS (0.50ms)** = **+46.6%**.

* [ ] **Fix P0 leak: `SnapshotStateObserver::fast_scopes` grows without bound**: in `snapshot_state_observer.rs` (`get_scope_entry()`), `fast_scopes: Vec<Option<Rc<ScopeEntry>>>` is indexed by `ScopeId` from a global `AtomicUsize` that only increases; every new scope does `fast_scopes.resize_with(id + 1, || None)` → Vec only grows. Fix: reclaim scope IDs (free-list / generation scheme) and/or reset per-composition; ensure scope teardown triggers cleanup.

* [ ] **Fix P0 leak: observer `scopes` accumulates because clear never happens**: `scopes: Vec<Rc<ScopeEntry>>` keeps entries; `clear(scope)` is never called from composition lifecycle. Fix: when `RecomposeScope` deactivates or its group ends, call `observer.clear(&scope)` (or equivalent) to remove entries; add generation-based cleanup.

* [ ] **Fix P0 leak: `StateArena::cells` never frees**: in `runtime.rs` (`StateArena::alloc()`), `cells: Vec<Option<Box<dyn AnyStateCell>>>` grows on every `mutableStateOf()/useState()`; `StateId` indices never reclaimed. Fix: add a free-list; when state drops push `StateId` to free list; `alloc()` reuses before pushing.

* [ ] **Fix P0 leak: dead watchers accumulate in `MutableStateInner::watchers`**: in `lib.rs` (~line **3020**, `subscribe_current_scope()`), dead `Weak<RecomposeScopeInner>` only cleaned via `retain()` when that state is read again; states written but not re-read can accumulate dead watchers. Fix: also clean on write/invalidation (ensure `invalidate_watchers()` retention is comprehensive, not only on upgrade path).

* [ ] **Remove P1 hot-path O(n) work on every state read**: `lib.rs` ~**3020** `subscribe_current_scope()` currently does `watchers.retain(...)` (GC scan O(n)) and then `watchers.iter().any(...)` (dedup O(n)) on every `.value()`/`.with()`. Fix: move GC to invalidation time and use O(1) dedup (e.g., `HashSet<ScopeId>`; or an epoch/tag scheme per scope).

* [ ] **Fix P1 O(n²) child diffing in `pop_parent()`**: `lib.rs` ~**2652** uses `current.iter().position(...)` inside a loop → O(n²); also allocates `HashSet<NodeId>` and clones vectors multiple times per diff. Fix: index-based diffing or real list-diff (LIS-based), preallocate scratch buffers, avoid `HashSet`/Vec clones on the hot path.

* [ ] **Fix P1 O(n²) scope grouping in `process_invalid_scopes()`**: `lib.rs` ~**3740** groups scopes via `scope_groups.iter_mut().find(|(existing, _)| Rc::ptr_eq(existing, &host))` (linear scan per scope). Fix: `HashMap` keyed by stable pointer identity (`Rc::as_ptr()`), O(1) grouping.

* [ ] **Cut allocation pressure: replace `Box<dyn FnMut>` commands with an enum**: `lib.rs` ~**1343** defines `type Command = Box<dyn FnMut(&mut dyn Applier)->...>`; every emit/move/insert/remove makes 1–4 heap allocations. Fix: `enum Command { InsertChild{...}, RemoveChild{...}, MoveChild{...}, UpdateNode{...}, BubbleDirty{...} }` stored inline in `Vec<Command>`.

* [ ] **Cut recomposition copying: make `snapshot_locals` COW**: `lib.rs` ~**330** clones locals every boundary: `*local_stack = stack.to_vec()`. Fix: store `Rc<Vec<LocalContext>>` (COW); only clone on write.

* [ ] **Fix pointer latency: stop cloning full hit-region list on every event**: `scene.rs` ~**395** does `let mut hits = self.hits.clone()` (clones all `HitRegion`s + nested vectors). Fix: collect references/indices only, sort refs/indices by z, return refs (or indices).

* [ ] **Stop double-storing hit regions**: `scene.rs` ~**329** clones into `node_index` and also pushes into `hits`. Fix: store hits once in `Vec<HitRegion>` and keep `HashMap<NodeId, usize>` to index into the Vec.

* [ ] **Avoid per-batch temporary Vec allocations in renderer**: `render.rs` ~**1330** collects batch slices into new Vecs each time (e.g., `shape_batch: Vec<&DrawShape> = ...collect()`). Fix: pass slice ranges `(start,end)` into encode functions, or reuse a scratch Vec cleared between batches.

* [ ] **Stop full scene rebuild for “any dirty bit”**: `app-shell/lib.rs` ~**1013** (`run_render_phase()`): any dirty flag (render/pointer/focus/cursor blink/draw repass) triggers `rebuild_scene_from_applier()` which walks whole tree and rebuilds all draw/hit vectors. Fix (short-term): split dirty flags so cursor blink/focus/pointer can skip scene rebuild and only rerender; (mid): dirty-subtree tracking; (long): retained scene graph/diff encoding (Vello-like).

* [ ] **Skip clean subtrees in layout box refresh**: `app-shell/lib.rs` ~**1061** `refresh_layout_box_data()` recursively walks entire `LayoutTree` even if only few nodes dirty and clones `Modifier` for dirty nodes. Fix: use `dirty_nodes: HashSet<NodeId>` (and propagate “subtree clean”) to avoid recursing into clean subtrees entirely.

* [ ] **Text is the main bottleneck: fix shaped-buffer reuse (telemetry proves it)**: in telemetry run `prepared_layout_cache_hit_rate=96.7%`, `size_hit_rate=78.3%`, but `text_cache_hit_rate=0.2%`, `reshape_rate=99.8%`, `reshapes=577`, `reuses=1`. That means cheap metadata caches work; expensive shaping cache is effectively failing.

* [ ] **Unify text identity across measure + render (currently incompatible)**: measurement uses content keys in `crates/cranpose-render/wgpu/src/lib.rs` at **:197** and **:1694**; rendering uses node keys in `crates/cranpose-render/wgpu/src/render.rs:2587`. Fix: carry `NodeId` into the text measurement API so measurement + render share the same identity for the same node and reuse becomes possible.

* [ ] **Re-architect text around node-local paragraph state**: one paragraph/prepared-layout object per text node; measurement, wrapping, offsets, and rendering all reuse *that same prepared paragraph*; stop using content-keyed measurement buffers as the primary identity.

* [ ] **Replace shared shaped-buffer cache `HashMap` + arbitrary trim with an LRU**: current shared text buffer cache is a global `HashMap` trimmed arbitrarily at **256** entries (`crates/cranpose-render/wgpu/src/lib.rs:794`), while size/prepared caches are LRUs (`lib.rs:1294`). Fix: use an LRU for shaped buffers too, with explicit caps + telemetry (occupancy/evictions).

* [ ] **Fix wrapped text reshaping even on “fast path”**: `crates/cranpose-render/wgpu/src/lib.rs:1459` wrapped measurement still calls `set_size`, `shape_until_scroll`, and invalidates cached size. `TextModifierNode` caches only final size-by-width (`crates/cranpose-ui/src/text_modifier_node.rs:83`), not a reusable prepared paragraph/layout. Fix: cache prepared wrapped layout/paragraph per node and constraint bucket (width) so wrap doesn’t re-shape constantly.

* [ ] **Do the zero/low-copy text wrapping rewrite**: in `crates/cranpose-ui/src/text/measure.rs`, functions like `wrap_line_greedy`, `split_annotated_lines`, `remap_annotated_for_display` clone `String`s and `Vec<RangeStyle>` heavily. Fix: operate on byte indices (`Range<usize>`) over the original buffers; only build final display string/ranges once wrap points are finalized.

* [ ] **Remove SipHash/`DefaultHasher` from internal hot hashes**: profiler shows hashing cost (e.g., **2.65% CPU**, mostly SipHash: ~**2.05%** `SipHasher::write` + **0.60%** `hash_one`). Replace `std::collections::hash_map::DefaultHasher` with your fast deterministic hasher everywhere internal:

  * `crates/cranpose-ui/src/text/style.rs:393` (`TextStyle::measurement_hash()`)
  * `crates/cranpose-foundation/src/modifier.rs:883`
  * `crates/cranpose-ui/src/modifier/mod.rs:243` (and noted `modifier/mod.rs` lines **244–292** hotspot area)
  * plus other hotspots called out: `scroll.rs`, `text_layout_result.rs`, `pointer_input.rs`
    Mechanical fix: use `cranpose_core::hash::default::DefaultHasher` (AHasher) + swap hot `HashMap`/`HashSet` to `FxHashMap`/`FxHashSet` where appropriate.

* [ ] **Flatten modifier/layout hot reads after text is fixed**: precompute layout/draw/input slices during modifier reconciliation so hot passes do fewer `RefCell` borrows and less delegate walking; targeted hotspots included `Modifier::eq_internal (~2.3% / or 0.75% in another profile)`, `ModifierChainNodeRef::with_state (~1.27%)`, `ModifierChainHandle::update_with_resolver (~1.25%)`, `LayoutBuilderState::measure_node (~1.39%)`, `SlotTable::remember (~1.18%)`. Relevant code refs:

  * `crates/cranpose-foundation/src/modifier.rs:2061`, `:1364`
  * `crates/cranpose-ui/src/layout/mod.rs:963`

* [ ] **Consider arena allocation for modifier nodes to reduce drop cost**: low-priority but real (`drop_in_place<ModifierKind> ~0.53%`); bump-allocate modifier nodes per frame/recompose to reduce allocator + drop overhead, improve locality.

* [ ] **Eliminate `Box<dyn Placeable>` allocations in layout (critical architectural win)**: `crates/cranpose-ui-layout/src/core.rs` defines `Measurable::measure -> Box<dyn Placeable>`, forcing heap alloc per node per measure pass. Fix: redesign `MeasureResult/Placeable` as concrete struct/enum/generics, or bump-allocate placeables for a pass and reset after the frame. Files to touch:

  * `crates/cranpose-ui-layout/src/core.rs`
  * `crates/cranpose-ui/src/layout/mod.rs`
  * `crates/cranpose-ui/src/widgets/nodes/layout_node.rs`

* [ ] **Replace `IndexSet<NodeId>` children storage with `Vec<NodeId>` where small**: `LayoutNode` uses `IndexSet<NodeId>` (`crates/cranpose-ui/src/widgets/nodes/layout_node.rs`), which adds hashing overhead on small lists; switch to `Vec<NodeId>` for ordered children (moves/removals by index are fine for small arrays).

* [ ] **Instrument renderer memory growth and cap caches**: generic harness shows memory growth even when scene rebuild clears per frame (`crates/cranpose-render/wgpu/src/lib.rs:1003` clears scene). Observed growth: size-tuned release **+317 MB**, `opt-level=3` **+301 MB**; other report calls it **708 MB / 10s** “critical”. First suspects to instrument: `TextAtlas` / `SwashCache` / shared text cache / image cache (e.g., `crates/cranpose-render/wgpu/src/render.rs:503`). Add logs for atlas growth, cache occupancy, evictions, per-frame retained bytes.

* [ ] **Tame `Mutex<FontSystem>` bottleneck for future parallelism**: render + measurement share a single `Arc<Mutex<FontSystem>>` (in `render.rs` / `lib.rs`); single-thread now but blocks future parallel layout. Fix later: `RwLock` or per-thread `FontSystem` instances; keep fast path lock-minimal.

* [ ] **Don’t spend cycles on lazy-list measurement yet**: telemetry shows ~**12 visible**, **16 total measured**, **~0.26–0.38ms**, `timed_out=false` → not the first fire.

* [ ] **Defer dependency-dup cleanup**: `cargo tree --duplicates` shows duplicates (`getrandom`, `smol_str`, `tiny-skia`, `ttf-parser`); worth cleaning later but not top speed lever.

* [ ] **Execution order (avoid random micro-opts)**: (1) split build profiles for native speed; (2) fix P0 leaks (scope observer + state arena + watcher cleanup); (3) fix state-read O(n) + pop_parent/process_invalid_scopes complexity; (4) re-architect text identity + node-local paragraph + LRU shaped buffers + wrapped layout caching; (5) rerun heavy markdown + generic harness;
