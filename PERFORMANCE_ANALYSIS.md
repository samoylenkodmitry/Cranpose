# Performance Analysis and Optimization Recommendations

**Date:** 2025-11-08
**Based on:** perf profile from desktop-app example

## Executive Summary

The performance profile reveals two critical application-level bottlenecks that could yield significant performance improvements:

1. **Text Rendering (20-25% of total CPU time)** - Text is being shaped twice per frame with no buffer sharing
2. **Layout Measurement (13%+ of total CPU time)** - Cache is invalidated every frame, causing full tree remeasurement

**Total optimization potential:** 40-60% reduction in application CPU time

---

## Performance Profile Breakdown

### Top Bottlenecks (Application Level)

| Component | % CPU Time | Type | Optimization Potential |
|-----------|------------|------|----------------------|
| Text shaping/rendering | 20-25% | Critical | HIGH - eliminate double shaping |
| GPU present (driver) | 32% | Unavoidable | LOW - driver overhead |
| Text measurement | 17% | Critical | HIGH - share buffers with rendering |
| Layout measurement | 13% | Critical | CRITICAL - fix cache invalidation |
| App update | 8.4% | Normal | MEDIUM - depends on app logic |

### Detailed Breakdown

```
Text-related (total ~42%):
├─ cosmic_text::buffer::Buffer::set_text .......... 23.50%
├─ cosmic_text::buffer::Buffer::shape_until ....... 22.37%
├─ measure_text operations ....................... 16.89%
├─ rustybuzz::plan::ShapePlan::new ................ 14.40%
├─ rustybuzz::shape::shape ........................ 11.79%
├─ rustybuzz::ot::map::MapBuilder::compile ........ 10.20%
└─ glyphon text preparation ....................... 2.33%

Layout (total ~13%):
└─ compose_ui::layout::LayoutBuilderState::measure_node ... 13.28%

Rendering (total ~10.3%):
└─ compose_render_wgpu::WgpuRenderer::render .............. 10.30%

Updates (total ~8.4%):
└─ compose_app_shell::AppShell<R>::update ................. 8.43%

GPU/Driver (total ~32%):
└─ wgpu::SurfaceTexture::present + NVIDIA driver .......... 32.23%
```

---

## Critical Issue #1: Double Text Shaping

### Problem

Text is being shaped **twice per frame** with no buffer sharing:

1. **During layout phase:** `WgpuTextMeasurer::measure()` shapes text to calculate size
2. **During render phase:** `GpuRenderer::render()` shapes the same text again for rendering

### Evidence from Codebase

**Measurement phase:**
- File: `crates/compose-render/wgpu/src/lib.rs:178-244`
- Creates and shapes `cosmic_text::Buffer` for measurement
- Has its own LRU cache (`buffer_cache`, capacity 64)

**Rendering phase:**
- File: `crates/compose-render/wgpu/src/render.rs:667-693`
- Creates and shapes `cosmic_text::Buffer` again for rendering
- Has separate HashMap cache (`text_cache`)

**No buffer sharing between phases!**

### Current Caching (What Works)

Both phases have good caching internally:

1. **WgpuTextMeasurer** (measurement):
   - Two-tier cache: size cache + buffer cache
   - Smart `ensure()` method avoids reshaping when text/font_size unchanged
   - LRU eviction

2. **GpuRenderer** (rendering):
   - HashMap-based buffer cache
   - Content-based key (text + scale, position-independent)
   - Smart `ensure()` method avoids reshaping when text/scale unchanged
   - Auto-cleanup of stale entries

**But the caches don't communicate!** The same text gets shaped in both caches.

### Root Cause

The `WgpuTextMeasurer` and `GpuRenderer` are separate structs with separate caches:

```rust
// lib.rs:162-176
pub struct WgpuTextMeasurer {
    buffer_cache: RefCell<LruCache<(String, i32), Box<MeasurementBuffer>>>,
    size_cache: RefCell<LruCache<(String, i32), Size>>,
}

// render.rs:288
pub struct GpuRenderer {
    text_cache: HashMap<TextKey, CachedTextBuffer>,  // Separate!
}
```

### Optimization: Share Buffer Cache

**Impact:** Eliminate ~50% of text shaping operations

**Implementation:**

1. **Create shared cache:**
```rust
// New shared cache type
type SharedTextCache = Arc<Mutex<HashMap<TextKey, CachedTextBuffer>>>;

// WgpuRenderer holds the shared cache
pub struct WgpuRenderer {
    shared_text_cache: SharedTextCache,
}

// WgpuTextMeasurer borrows reference
pub struct WgpuTextMeasurer {
    shared_text_cache: SharedTextCache,
    size_cache: RefCell<LruCache<(String, i32), Size>>,
}
```

2. **Modify measurement to populate shared cache:**
   - File: `crates/compose-render/wgpu/src/lib.rs:178-244`
   - Check shared cache first
   - If miss, shape and store in shared cache
   - Size cache remains local (fast path)

3. **Modify rendering to use shared cache:**
   - File: `crates/compose-render/wgpu/src/render.rs:667-693`
   - Already checks cache, just use the shared one

**Expected reduction:** 10-12% of total CPU time (half of 20-25%)

---

## Critical Issue #2: Layout Cache Invalidated Every Frame

### Problem

The layout measurement cache is **completely invalidated every single frame**, causing the entire UI tree to be remeasured from scratch even when nothing changed.

### Evidence from Codebase

**Cache structure:**
- File: `crates/compose-ui/src/widgets/nodes/layout_node.rs:59-68`
- Each `LayoutNode` has `LayoutNodeCacheHandles` with:
  - `measurements: Vec<MeasurementCacheEntry>` - cached by constraints
  - `epoch: u64` - cache generation number

**Cache invalidation:**
- File: `crates/compose-ui/src/layout/mod.rs:325`
```rust
pub fn measure_layout(...) -> Result<...> {
    let builder = LayoutBuilderState::new(
        applier,
        cache_epoch: NEXT_CACHE_EPOCH.fetch_add(1, Ordering::Relaxed),  // NEW EPOCH!
        max_size,
    );
}
```

**Every frame:**
```rust
// AppShell::run_layout_phase calls measure_layout
// → Creates new LayoutBuilderState with new epoch
// → All cached measurements have old epoch
// → All cache lookups miss!
```

### Impact

The cache exists and has proper lookup logic:

```rust
// layout_node.rs:534
if let Some(cached) = cache.get_measurement(constraints) {
    return Ok(cached);  // Never happens across frames!
}
```

But `get_measurement()` checks epoch:
```rust
// layout_node.rs:87-93
if state.epoch != self.epoch {
    return None;  // Cache miss every frame!
}
```

**Result:** Full tree remeasurement every frame, even for completely static UI.

### Optimization: Preserve Cache Across Frames

**Impact:** 50-90% reduction in layout measurement time for typical updates

**Approach 1: Selective Invalidation (Recommended)**

Integrate with Compose's existing invalidation tracking:

1. **Track which nodes changed during recomposition:**
   - Compose already tracks invalidated nodes
   - Propagate invalidation info to layout phase

2. **Per-node epochs or dirty flags:**
```rust
pub struct LayoutNodeCacheHandles {
    measurements: Vec<MeasurementCacheEntry>,
    dirty: bool,  // Set during recomposition
}

// In measure_layout_node:
if cache.dirty {
    cache.measurements.clear();  // Invalidate this node only
    cache.dirty = false;
}
```

3. **Subtree invalidation:**
   - When a node is dirty, mark its children's caches dirty too
   - Only remeasure dirty subtrees

**Approach 2: Global Epoch with Change Detection (Simpler)**

Only increment epoch when composition actually changed:

```rust
pub fn measure_layout(
    applier: &mut MemoryApplier,
    root: NodeId,
    max_size: Size,
    composition_changed: bool,  // NEW parameter
) -> Result<...> {
    let epoch = if composition_changed {
        NEXT_CACHE_EPOCH.fetch_add(1, Ordering::Relaxed)
    } else {
        NEXT_CACHE_EPOCH.load(Ordering::Relaxed)  // Reuse
    };
}
```

**Expected reduction:** 8-10% of total CPU time for static content

---

## Medium Impact Optimizations

### 3. Cache Size in MeasurementBuffer

**Problem:**
- File: `crates/compose-render/wgpu/src/lib.rs:228-235`
- Size is recalculated from buffer layout runs on every buffer cache hit

**Solution:**
```rust
// lib.rs:126-131
struct MeasurementBuffer {
    buffer: Buffer,
    text: String,
    font_size: f32,
    cached_size: Option<Size>,  // NEW
}

// Only recalculate when ensure() returns true (reshaping occurred)
```

**Expected impact:** 1-2% reduction

### 4. Smarter Rendering Cache Eviction

**Problem:**
- File: `crates/compose-render/wgpu/src/render.rs:655-656`
- ALL text cache entries not in current frame are evicted
- Temporarily hidden text (scrolled offscreen, tab switch) must be reshaped

**Solution:**
```rust
// Add generation counter to cache entries
struct CachedTextBuffer {
    buffer: Buffer,
    text: String,
    scale: f32,
    last_used_frame: u64,  // NEW
}

// Keep for N frames (e.g., 60 = 1 second at 60fps)
text_cache.retain(|key, entry| {
    current_frame - entry.last_used_frame < 60
});
```

**Expected impact:** 1-3% reduction for UIs with off-screen text

### 5. Optimize Layout Cache Data Structure

**Problem:**
- File: `crates/compose-ui/src/widgets/nodes/layout_node.rs:87-93`
- Cache lookup is O(n) linear search: `measurements.iter().find(...)`

**Solution:**
```rust
// Implement Hash for Constraints (already has PartialEq)
impl Hash for Constraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.min_width.to_bits().hash(state);
        self.max_width.to_bits().hash(state);
        self.min_height.to_bits().hash(state);
        self.max_height.to_bits().hash(state);
    }
}

// Change to HashMap
measurements: HashMap<Constraints, Rc<MeasuredNode>>  // O(1) lookup
```

**Expected impact:** 1-2% reduction (but moot if cache invalidated every frame!)

---

## Low Impact Optimizations

### 6. Fix Inefficient Measurement Loop

**Problem:**
- File: `crates/compose-render/wgpu/src/lib.rs:228-235`
```rust
for _line in buffer.lines.iter() {  // Loops over all lines
    let layout_runs = buffer.layout_runs();
    for run in layout_runs {
        max_width = max_width.max(run.line_w);
        break;  // But always breaks after first!
    }
    total_height += font_size * 1.4;
}
```

**Issue:** Outer loop variables are unused, should calculate height properly

**Solution:**
```rust
let layout_runs = buffer.layout_runs();
let mut max_width = 0.0f32;
for run in layout_runs {
    max_width = max_width.max(run.line_w);
}
let total_height = buffer.lines.len() as f32 * font_size * 1.4;
```

**Expected impact:** <1% reduction

---

## Implementation Priority

### Phase 1: Critical Fixes (Target: 20-30% total CPU reduction)

1. **Fix layout cache invalidation** (Priority: CRITICAL)
   - Files: `crates/compose-ui/src/layout/mod.rs`
   - Expected: 8-10% reduction
   - Complexity: Medium
   - Risk: Medium (changes core layout logic)

2. **Share text buffer cache** (Priority: HIGH)
   - Files: `crates/compose-render/wgpu/src/lib.rs`, `src/render.rs`
   - Expected: 10-12% reduction
   - Complexity: Medium
   - Risk: Low (isolated to renderer)

### Phase 2: Medium Impact (Target: 3-5% additional reduction)

3. Cache size in MeasurementBuffer
4. Smarter rendering cache eviction
5. Optimize layout cache data structure

### Phase 3: Polish (Target: 1-2% additional reduction)

6. Fix measurement loop inefficiency
7. Pre-warm common text
8. Other micro-optimizations

---

## Benchmarking Strategy

**Before any changes:**
1. Run existing benchmarks: `cargo bench --package compose-ui -- pipeline`
2. Record baseline measurements
3. Profile with perf again for detailed comparison

**After each optimization:**
1. Re-run benchmarks
2. Compare perf profiles
3. Verify no regressions
4. Document actual vs expected improvements

**Existing benchmark file:**
- `crates/compose-ui/benches/pipeline.rs`
- Already measures composition, measurement, layout, rendering separately

---

## Files to Modify

### Phase 1 (Critical)

**Layout cache:**
- `crates/compose-ui/src/layout/mod.rs` (lines 325, 343-392)
- `crates/compose-ui/src/widgets/nodes/layout_node.rs` (lines 59-68, 87-93, 534)
- `crates/compose-app-shell/src/lib.rs` (integrate invalidation tracking)

**Text buffer sharing:**
- `crates/compose-render/wgpu/src/lib.rs` (lines 162-176, 178-244)
- `crates/compose-render/wgpu/src/render.rs` (lines 288, 667-693)

### Phase 2 (Medium)

**Size caching:**
- `crates/compose-render/wgpu/src/lib.rs` (lines 126-159, 224-244)

**Cache eviction:**
- `crates/compose-render/wgpu/src/render.rs` (lines 655-656)

**HashMap cache:**
- `crates/compose-ui-layout/src/constraints.rs` (add Hash impl)
- `crates/compose-ui/src/widgets/nodes/layout_node.rs` (change to HashMap)

---

## Summary of Expected Improvements

| Optimization | CPU Time Saved | Complexity | Risk |
|--------------|----------------|------------|------|
| Fix layout cache invalidation | 8-10% | Medium | Medium |
| Share text buffer cache | 10-12% | Medium | Low |
| Cache size in MeasurementBuffer | 1-2% | Low | Low |
| Smarter cache eviction | 1-3% | Low | Low |
| HashMap for cache lookup | 1-2% | Low | Low |
| Fix measurement loop | <1% | Low | Low |
| **TOTAL POTENTIAL** | **22-30%** | | |

**Conservative estimate:** 20-25% reduction in application CPU time
**Optimistic estimate:** 30-40% reduction with follow-up optimizations

---

## Next Steps

1. **Benchmark baseline** - Establish current performance metrics
2. **Implement Phase 1** - Critical fixes for maximum impact
3. **Re-benchmark** - Validate improvements
4. **Implement Phase 2** - Medium impact optimizations
5. **Profile again** - Look for new bottlenecks revealed by optimizations
6. **Consider GPU optimizations** - Once CPU bottlenecks are resolved

## Notes

- The 32% spent in GPU present/driver is largely unavoidable but could be investigated if CPU optimizations reveal GPU bottlenecks
- Text shaping is expensive by nature (rustybuzz, font metrics) but avoiding duplicate shaping is the key win
- The layout system architecture is solid; it just needs proper cache preservation
- All recommendations preserve the existing API and architecture
