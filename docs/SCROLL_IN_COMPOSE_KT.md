# Scroll behavior in Jetpack Compose: source notes

This document records a source review of Jetpack Compose scroll behavior. The
Source revision: unspecified. The Kotlin excerpts and class details serve as a
historical architecture reference. Current
Cranpose scroll behavior lives in
[`scroll.rs`](../crates/cranpose-ui/src/modifier/scroll.rs),
[`lazy_list_doc.md`](lazy_list_doc.md) and
[`lazy_list_state.rs`](../crates/cranpose-foundation/src/lazy/lazy_list_state.rs).

## Table of Contents
1. [Executive Summary](#executive-summary)
2. [Architecture Overview](#architecture-overview)
3. [Input Event Propagation](#input-event-propagation)
4. [Scroll State Management](#scroll-state-management)
5. [Layout Offset Application](#layout-offset-application)
6. [Complete Flow: Touch to Visual Movement](#complete-flow-touch-to-visual-movement)
7. [Key Classes Reference](#key-classes-reference)

---

## Executive Summary

Compose's scrolling system consists of **three interconnected layers**:

1. **Input Layer** (`Modifier.scrollable`): Handles touch events, gestures, and drag detection
2. **State Layer** (`ScrollState`): Manages scroll position and delegates to input system
3. **Layout Layer** (`ScrollNode`): Reads scroll state and applies physical offset to content

**Critical Insight:** `Modifier.scrollable()` alone **does NOT move content**. It only handles input and updates state. You need `Modifier.verticalScroll()` or `Modifier.horizontalScroll()` which combine `scrollable()` + `ScrollNode` layout modifier to actually offset content.

---

## Architecture Overview

```mermaid
graph TD
    A[User Touch/Drag] --> B[PointerInputEventProcessor]
    B --> C[HitPathTracker - 3 Pass Dispatch]
    C --> D[ScrollableNode PointerInputModifierNode]
    D --> E[DragGestureNode - Drag Detection]
    E --> F[ScrollingLogic.scrollByWithOverscroll]
    F --> G[NestedScrollDispatcher - Pre/Post Scroll]
    G --> H[ScrollableState.scrollBy]
    H --> I[ScrollState.value Update]
    I --> J{Snapshot System}
    J --> K[Recomposition Triggered]
    K --> L[ScrollNode.measure]
    L --> M[Read state.value]
    M --> N[Calculate offset: -state.value]
    N --> O[placeable.placeRelativeWithLayer xOffset, yOffset]
    O --> P[Content Visually Scrolled!]
```

---

## Input Event Propagation

### 1. **PointerInputEventProcessor** - Entry Point
**Location:** `compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer/PointerInputEventProcessor.kt`

**Role:** Receives platform input events and orchestrates hit testing + dispatch.

```kotlin
fun process(
    pointerEvent: PointerInputEvent,
    positionCalculator: PositionCalculator,
    isInBounds: Boolean = true
): ProcessResult
```

**Flow:**
1. Convert `PointerInputEvent` → `InternalPointerEvent` (tracking deltas)
2. For pointer down/hover: perform `root.hitTest(position, hitResult, type)`
3. Add hit modifiers to `HitPathTracker`
4. Dispatch via `hitPathTracker.dispatchChanges()`

---

### 2. **HitPathTracker** - Event Dispatcher
**Location:** `compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer/HitPathTracker.kt`

**Role:** Maintains tree of hit modifier nodes and coordinates three-pass event dispatch.

**Three-Pass System:**

#### Pass 1: **Initial** (Parent → Child - Tunneling)
- Parents get first chance to intercept
- Can consume before children see event
- Example: Scrollable can prevent child button from handling

#### Pass 2: **Main** (Child → Parent - Bubbling)  
- Children handle first
- Parents process unconsumed events
- **This is where scrolling typically happens**

#### Pass 3: **Final** (Parent → Child - Tunneling)
- Final observation pass
- Even if consumed, parents can observe
- Used for analytics/completion tracking

**Critical Architecture:**
```kotlin
// HitPathTracker.Node - represents one modifier in hit path
class Node(val modifierNode: Modifier.Node) : NodeParent() {
    // Caches per-dispatch
    private var coordinates: LayoutCoordinates?  // For position transformation
    private var pointerEvent: PointerEvent?       // Event in local coordinates
    private val relevantChanges: LongSparseArray<PointerInputChange>
    
    override fun dispatchMainEventPass(...) {
        // Initial pass - parent first
        modifierNode.onPointerEvent(event, PointerEventPass.Initial, size)
        
        // Recurse to children
        children.forEach { it.dispatchMainEventPass(...) }
        
        // Main pass - parent after children
        modifierNode.onPointerEvent(event, PointerEventPass.Main, size)
    }
}
```

**Position Transformation:**
Every node receives events in its local coordinate space:
```kotlin
// In buildCache()
change.copy(
    currentPosition = coordinates!!.localPositionOf(parentCoordinates, currentPosition),
    previousPosition = coordinates!!.localPositionOf(parentCoordinates, prevPosition)
)
```

---

### 3. **SuspendingPointerInputModifierNode** - Coroutine-Based Input
**Location:** `compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/input/pointer/SuspendingPointerInputFilter.kt`

**Role:** Enables `Modifier.pointerInput { }` for suspending gesture detection.

**Key Features:**
- Lazy coroutine launch on first event
- Multiple concurrent `awaitPointerEventScope` blocks
- **Synchronous event resumption** (critical for consumption to work)

```kotlin
@RestrictsSuspension
interface AwaitPointerEventScope {
    suspend fun awaitPointerEvent(pass: PointerEventPass = Main): PointerEvent
}

// Usage in gesture detector:
awaitPointerEventScope {
    val down = awaitFirstDown()  // Suspends until pointer down
    drag(down.id) { change ->
        change.consume()  // Consume immediately
        onDelta(change.positionChange())
    }
}
```

**Why Synchronous?**
- No dispatch delay between event arrival and handler execution
- Allows modifying `PointerInputChange` before next stage sees it
- Essential for proper event consumption across modifier chain

---

### 4. **ScrollableNode** - Scrollable Input Handler
**Location:** `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/gestures/Scrollable.kt`

**Role:** Handles scrolling input (touch drag, mouse wheel) and delegates to `ScrollingLogic`.

**Inheritance Chain:**
```kotlin
class ScrollableNode : DragGestureNode, KeyInputModifierNode, SemanticsModifierNode
```

**Delegation:**
- `DragGestureNode`: Provides drag detection via pointer input
- `MouseWheelScrollingLogic`: Handles mouse wheel events
- `ScrollingLogic`: Core scrolling operations + nested scroll
- `nestedScrollModifierNode`: Nested scroll coordinator
- `ContentInViewNode`: Focus-based scrolling
- `ScrollableContainerNode`: Marks as scrollable for hierarchical queries

**Event Processing:**
```kotlin
override fun onPointerEvent(pointerEvent: PointerEvent, pass: PointerEventPass, bounds: IntSize) {
    // Handle draggable events (touch)
    if (pointerEvent.changes.fastAny { canDrag.invoke(it.type) }) {
        super.onPointerEvent(pointerEvent, pass, bounds)  // DragGestureNode handles
    }
    // Handle mouse wheel
    if (enabled \u0026\u0026 pass == Initial \u0026\u0026 pointerEvent.type == Scroll) {
        ensureMouseWheelScrollNodeInitialized()
    }
    mouseWheelScrollingLogic?.onPointerEvent(pointerEvent, pass, bounds)
}
```

**Drag Handling:**
```kotlin
override suspend fun drag(forEachDelta: suspend ((dragDelta: DragEvent.DragDelta) -> Unit) -> Unit) {
    with(scrollingLogic) {
        scroll(scrollPriority = MutatePriority.UserInput) {
            forEachDelta { dragDelta ->
                // Account for indirect pointer events (trackpad, etc)
                val invertIndirectPointer = if (dragDelta.isIndirectPointerEvent) -1f else 1f
                scrollByWithOverscroll(
                    dragDelta.delta.singleAxisOffset() * invertIndirectPointer,
                    source = UserInput
                )
            }
        }
    }
}
```

**Key Point:** `ScrollableNode` **does not move content**. It only:
1. Detects drag gestures
2. Calls `scrollingLogic.scrollByWithOverscroll()`
3. Updates `ScrollableState`

---

### 5. **ScrollingLogic** - Core Scroll Operations
**Location:** Inside `Scrollable.kt`

**Role:** Coordinates nested scrolling, overscroll, and delegates to `ScrollableState`.

**Nested Scroll Flow:**
```kotlin
private fun ScrollScope.performScroll(delta: Offset, source: NestedScrollSource): Offset {
    // Step 1: Pre-scroll - parent gets first chance
    val consumedByPreScroll = nestedScrollDispatcher.dispatchPreScroll(delta, source)
    
    // Step 2: Self-scroll - this scrollable consumes
    val scrollAvailableAfterPreScroll = delta - consumedByPreScroll
    val singleAxisDelta = scrollAvailableAfterPreScroll.singleAxisOffset().reverseIfNeeded()
    
    val consumedBySelfScroll = scrollBy(singleAxisDelta.toFloat())  // <-- Updates state here
        .toOffset()
        .reverseIfNeeded()
    
    // Step 3: Notify layout system for invalidation
    onScrollChangedDispatcher.dispatchScrollDeltaInfo(consumedBySelfScroll)
    
    // Step 4: Post-scroll - parent handles leftovers  
    val deltaAvailableAfterScroll = scrollAvailableAfterPreScroll - consumedBySelfScroll
    val consumedByPostScroll = nestedScrollDispatcher.dispatchPostScroll(
        consumedBySelfScroll,
        deltaAvailableAfterScroll,
        source
    )
    
    return consumedByPreScroll + consumedBySelfScroll + consumedByPostScroll
}
```

**Overscroll Wrapper:**
```kotlin
fun NestedScrollScope.scrollByWithOverscroll(offset: Offset, source: NestedScrollSource): Offset {
    val overscroll = overscrollEffect
    return if (overscroll != null \u0026\u0026 shouldDispatchOverscroll) {
        overscroll.applyToScroll(offset, source) { delta ->
            performScroll(delta, source)  // Wrapped
        }
    } else {
        performScroll(offset, source)
    }
}
```

**Fling Handling:**
```kotlin
suspend fun onScrollStopped(initialVelocity: Velocity, isMouseWheel: Boolean) {
    val performFling: suspend (Velocity) -> Velocity = { velocity ->
        // 1. Pre-fling (parent consumes first)
        val preConsumed = nestedScrollDispatcher.dispatchPreFling(velocity)
        val available = velocity - preConsumed
        
        // 2. Self-fling animation
        val velocityLeft = doFlingAnimation(available)
        
        // 3. Post-fling (parent handles leftover)
        nestedScrollDispatcher.dispatchPostFling(available - velocityLeft, velocityLeft)
        
        velocity - velocityLeft
    }
    
    overscrollEffect?.applyToFling(availableVelocity, performFling) ?: performFling(availableVelocity)
}
```

```kotlin
override suspend fun doFlingAnimation(available: Velocity): Velocity {
    scroll(scrollPriority = MutatePriority.Default) {
        with(flingBehavior) {
            performFling(available.toFloat())  // Animates scrollBy() calls
        }
    }
}
```

---

## Scroll State Management

### 6. **ScrollState** - Scroll Position State
**Location:** `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/Scroll.kt`

**Role:** Holds current scroll position and implements `ScrollableState` interface.

**Structure:**
```kotlin
@Stable
class ScrollState(initial: Int) : ScrollableState {
    // Current scroll position in pixels
    var value: Int by mutableIntStateOf(initial)
        private set
    
    // Maximum scroll (content size - viewport size)
    var maxValue: Int
        get() = _maxValueState.intValue
        internal set(newMax) {
            _maxValueState.intValue = newMax
            if (value > newMax) value = newMax
        }
    
    // Viewport size (visible area)
    var viewportSize: Int by mutableIntStateOf(0)
        internal set
    
    // Total content size
    internal var contentSize by mutableIntStateOf(0)
    
    // Fractional scroll accumulator (events are floats, value is int)
    private var accumulator: Float = 0f
    
    // The actual state implementation
    private val scrollableState = ScrollableState { delta ->
        val absolute = (value + delta + accumulator)
        val newValue = absolute.coerceIn(0f, maxValue.toFloat())
        val changed = absolute != newValue
        val consumed = newValue - value
        val consumedInt = consumed.fastRoundToInt()
        value += consumedInt  // <-- STATE UPDATE HAPPENS HERE
        accumulator = consumed - consumedInt
        
        if (changed) consumed else delta  // Return consumed amount
    }
}
```

**Key Methods:**
```kotlin
// Immediate jump
suspend fun scrollTo(value: Int): Float = 
    this.scrollBy((value - this.value).toFloat())

// Animated scroll
suspend fun animateScrollTo(value: Int, animationSpec: AnimationSpec<Float> = SpringSpec()) =
    this.animateScrollBy((value - this.value).toFloat(), animationSpec)

// ScrollableState delegation
override suspend fun scroll(scrollPriority: MutatePriority, block: suspend ScrollScope.() -> Unit) =
    scrollableState.scroll(scrollPriority, block)

override fun dispatchRawDelta(delta: Float): Float = 
    scrollableState.dispatchRawDelta(delta)
```

**State Update Mechanism:**
```kotlin
// When scrollBy() is called from ScrollingLogic:
scrollableState { delta ->
    val newValue = (value + delta).coerceIn(0f, maxValue.toFloat())
    val consumed = newValue - value
    value += consumed.roundToInt()  // Triggers Compose recomposition
    return consumed
}
```

**When is `value` updated?**
1. During event processing in the **Main pass**
2. Inside `ScrollingLogic.performScroll()` → `scrollBy()` → `scrollableState { }`
3. **Synchronously** during the same frame as the input event
4. **But UI updates asynchronously** in the next composition/layout pass

---

## Layout Offset Application

### 7. **ScrollNode** - Layout Modifier That Moves Content
**Location:** Inside `Scroll.kt`

**Role:** Reads `ScrollState.value` during layout and applies physical offset to content.

**Critical: This is WHO READS the scroll state and MOVES the content!**

```kotlin
internal class ScrollNode(
    var state: ScrollState,
    var reverseScrolling: Boolean,
    var isVertical: Boolean
) : LayoutModifierNode, SemanticsModifierNode, Modifier.Node() {
    
    override fun MeasureScope.measure(
        measurable: Measurable,
        constraints: Constraints
    ): MeasureResult {
        // Step 1: Allow content to be larger than viewport
        val childConstraints = constraints.copy(
            maxHeight = if (isVertical) Constraints.Infinity else constraints.maxHeight,
            maxWidth = if (isVertical) constraints.maxWidth else Constraints.Infinity
        )
        
        // Step 2: Measure child with infinite constraints on scroll axis
        val placeable = measurable.measure(childConstraints)
        
        // Step 3: Calculate actual viewport size
        val width = placeable.width.coerceAtMost(constraints.maxWidth)
        val height = placeable.height.coerceAtMost(constraints.maxHeight)
        
        // Step 4: Calculate maximum scroll distance
        val scrollHeight = placeable.height - height
        val scrollWidth = placeable.width - width
        val side = if (isVertical) scrollHeight else scrollWidth
        
        // Step 5: Update state with layout measurements
        state.maxValue = side  // <-- Tells state max scroll
        state.viewportSize = if (isVertical) height else width
        state.contentSize = if (isVertical) placeable.height else placeable.width
        
        // Step 6: Layout and place with offset
        return layout(width, height) {
            // READ STATE VALUE HERE
            val scroll = state.value.fastCoerceIn(0, side)
            
            // Calculate offset (negative for scroll down/right)
            val absScroll = if (reverseScrolling) scroll - side else -scroll
            val xOffset = if (isVertical) 0 else absScroll
            val yOffset = if (isVertical) absScroll else 0
            
            // APPLY OFFSET - This moves the content!
            withMotionFrameOfReferencePlacement {
                placeable.placeRelativeWithLayer(xOffset, yOffset)
            }
        }
    }
}
```

**Key Insights:**

1. **Infinite Constraints:** Child can measure to full content size
   ```kotlin
   maxHeight = if (isVertical) Constraints.Infinity else constraints.maxHeight
   ```

2. **State Updates in Measure:** `maxValue`, `viewportSize`, `contentSize` updated before placement
   ```kotlin
   state.maxValue = placeable.height - height
   ```

3. **State Value Read:** During placement lambda
   ```kotlin
   val scroll = state.value  // Reads observable state
   ```

4. **Offset Calculation:**
   ```kotlin
   // For vertical scroll, scrolling down (value increases) moves content UP
   val absScroll = -scroll  // Negative offset
   val yOffset = absScroll  // Applied as negative Y offset
   ```

5. **Placement:** Uses `placeRelativeWithLayer()` for efficient offset
   ```kotlin
   placeable.placeRelativeWithLayer(xOffset, yOffset)
   ```

**`withMotionFrameOfReferencePlacement`:**
- Tags offset as "direct manipulation"
- Allows consumers to decide animation behavior
- E.g., `Modifier.approachLayout()` can skip animating scroll offsets

---

## Complete Flow: Touch to Visual Movement

### Scenario: User Scrolls a Column

```kotlin
Column(
    modifier = Modifier
        .size(300.dp)
        .verticalScroll(rememberScrollState())
) {
    // ... 1000.dp of content
}
```

**Expanded Modifier Chain:**
```kotlin
Modifier.verticalScroll(state) = 
    Modifier
        .scrollableArea(state, Vertical, ...)  // Contains scrollable() + clip + overscroll
        .then(ScrollingLayoutElement(state, false, true))  // Creates ScrollNode
        
// scrollableArea further expands to:
Modifier
    .scrollable(state, Vertical, ...)  // Creates ScrollableNode
    .clip(...)
    .overscroll(...)
```

**Complete Timeline:**

#### Frame N: User Touches and Drags Down

**1. Platform Event → PointerInputEventProcessor**
```
Platform: MotionEvent(ACTION_MOVE, x=150, y=200, delta=(0, -10))
  ↓
PointerInputEventProcessor.process(PointerInputEvent)
  ↓ Creates InternalPointerEvent with deltas
```

**2. Hit Testing (First Touch Only)**
```
PointerInputEventProcessor.process()
  → root.hitTest(position=(150, 200), hitResult, Touch)
  → Finds: [ScrollableNode, ...other modifiers...]
  → hitPathTracker.addHitPath(pointerId, [ScrollableNode, ...])
```

**3. Three-Pass Event Dispatch**
```
HitPathTracker.dispatchChanges(internalPointerEvent):

buildCache():
  → Transform position to ScrollableNode local coordinates
  → Create PointerEvent with local positions
  → Cache in Node

INITIAL PASS (Parent → Child):
  → ScrollableNode.onPointerEvent(event, Initial, size)
  → ScrollableNode doesn't consume in Initial pass
  → (No children to recurse to)

MAIN PASS (Child → Parent):
  → (No children)
  → ScrollableNode.onPointerEvent(event, Main, size)
  → DragGestureNode's pointer input handler runs:
  
    awaitPointerEventScope {
        drag(pointerId) { change ->
            change.consume()  // <-- Consumes event
            onDelta(change.positionChange())  // (0, -10)
        }
    }
  
  → Calls ScrollableNode.drag() callback:
  
    suspend fun drag(forEachDelta) {
        scrollingLogic.scroll(MutatePriority.UserInput) {
            forEachDelta { dragDelta ->
                scrollByWithOverscroll(
                    delta = (0, -10).singleAxisOffset(),  // (0, -10) for vertical
                    source = UserInput
                )
            }
        }
    }

FINAL PASS (Parent → Child):
  → ScrollableNode.onPointerEvent(event, Final, size)
  → (Typically no-op for scrollable)
```

**4. Scroll Logic Execution**
```
ScrollingLogic.scrollByWithOverscroll((0, -10)):
  
  → overscrollEffect?.applyToScroll((0, -10), UserInput) { delta ->
      performScroll(delta, UserInput)
  }
  
  → performScroll((0, -10), UserInput):
  
    // Nested scroll pre-scroll
    consumed1 = nestedScrollDispatcher.dispatchPreScroll((0, -10), UserInput)
    // Assume no parent scrollable: consumed1 = (0, 0)
    
    available = (0, -10) - (0, 0) = (0, -10)
    
    // Self scroll (single axis, reversed)
    // singleAxisOffset() → (0, -10)
    // reverseIfNeeded() → (0, 10)  [natural scrolling]
    // toFloat() → 10.0
    
    consumed2 = scrollBy(10.0)  // <-- Calls ScrollableState
    
    → ScrollState.scrollableState { delta=10.0 ->
        val newValue = (value + 10.0).coerceIn(0, maxValue)
        // If value was 50, maxValue 700:
        val newValue = 60.coerceIn(0, 700) = 60
        val consumed = 60 - 50 = 10
        value = 60  // <<< STATE UPDATE - TRIGGERS RECOMPOSITION
        return 10.0
    }
    
    consumed2 = (0, 10)  // Back to offset
    
    // Notify for invalidation
    onScrollChangedDispatcher.dispatchScrollDeltaInfo((0, 10))
    
    // Nested scroll post-scroll
    leftover = (0, -10) - (0, 10) = (0, 0)
    consumed3 = nestedScrollDispatcher.dispatchPostScroll((0, 10), (0, 0), UserInput)
    // consumed3 = (0, 0)
    
    return (0, 0) + (0, 10) + (0, 0) = (0, 10)
```

**5. State Changed → Snapshot System**
```
ScrollState.value changed from 50 to 60
  ↓
Compose snapshot system detects state write
  ↓
Marks composition as invalid
  ↓
Schedules recomposition for next frame
```

**The input phase completes here. The next layout and draw pass shows the updated position.**

---

#### Frame N+1: Recomposition and Layout

**6. Recomposition Triggered**
```
Compose framework:
  → Snapshot.sendApplyNotifications()
  → Recomposition scheduled
  → Column recomposes (if needed)
  → Modifier chain re-evaluated
```

**7. Layout Pass - ScrollNode.measure()**
```
ScrollNode.measure():
  
  // Child constraints: infinite height
  childConstraints = Constraints(
    maxWidth = 300.dp.toPx(),
    maxHeight = Infinity
  )
  
  // Measure child
  placeable = measurable.measure(childConstraints)
  // placeable.height = 1000.dp.toPx() (full content)
  
  // Viewport size
  width = placeable.width.coerceAtMost(300.dp.toPx())
  height = placeable.height.coerceAtMost(300.dp.toPx())
  // width = 300.dp, height = 300.dp
  
  // Max scroll
  scrollHeight = 1000.dp - 300.dp = 700.dp
  state.maxValue = 700.dp.toPx()
  state.viewportSize = 300.dp.toPx()
  state.contentSize = 1000.dp.toPx()
  
  return layout(width, height) {
    // READ STATE VALUE
    val scroll = state.value  // 60px
    
    // Calculate offset
    val absScroll = -scroll  // -60px
    val yOffset = absScroll  // -60px
    
    // PLACE WITH OFFSET - CONTENT MOVES!
    placeable.placeRelativeWithLayer(xOffset=0, yOffset=-60)
  }
```

**8. Content Positioned**
```
Before scroll: content at (0, 0)
After scroll:  content at (0, -60)

Visual effect: Content shifted UP 60px
User perception: Scrolled DOWN 60px
```

**9. Draw Pass**
```
Compose draws content at new position
  → Content clipped to viewport bounds (300.dp x 300.dp)
  → User sees content shifted by 60px
```

---

### Summary of the Chain

```
Touch Input
  ↓
PointerInputEventProcessor (hit test + dispatch)
  ↓
HitPathTracker (3-pass dispatch to modifiers)
  ↓
ScrollableNode.onPointerEvent (receives event)
  ↓
DragGestureNode (detects drag, consumes event)
  ↓
ScrollableNode.drag() callback
  ↓
ScrollingLogic.scrollByWithOverscroll()
  ↓
ScrollingLogic.performScroll() (nested scroll coordination)
  ↓
ScrollableState.scrollBy()
  ↓
ScrollState.scrollableState { } lambda
  ↓
ScrollState.value = newValue  ← STATE WRITE
  ↓
[Snapshot system detects change]
  ↓
[Next Frame]
  ↓
Recomposition triggered
  ↓
ScrollNode.measure() executed
  ↓
state.value READ during placement
  ↓
placeable.placeRelativeWithLayer(x, -state.value)  ← CONTENT MOVED
  ↓
Draw pass renders at new position
  ↓
User sees scroll!
```

---

## Key Differences: `Modifier.scrollable()` vs `Modifier.verticalScroll()`

### `Modifier.scrollable(state, Vertical)`

**What it does:**
- ✅ Handles touch input and drag detection
- ✅ Updates `ScrollableState.value`
- ✅ Nested scroll coordination
- ✅ Fling behavior
- ✅ Overscroll effects
- ❌ **Does NOT move content**
- ❌ **Does NOT clip to viewport**
- ❌ **Does NOT measure child with infinite constraints**

**When to use:**
- Custom scroll logic where you handle layout yourself
- Want to react to scroll delta without standard scroll behavior
- Building custom scrollable components

**Example:**
```kotlin
var offset by remember { mutableStateOf(0f) }
val scrollState = rememberScrollState()

Box(
    modifier = Modifier
        .scrollable(
            state = scrollState,
            orientation = Orientation.Vertical
        )
) {
    // Content does NOT move automatically
    // You must read scrollState.value and apply offset yourself
}
```

---

### `Modifier.verticalScroll(state)` / `Modifier.horizontalScroll(state)`

**What it does:**
- ✅ Everything `scrollable()` does
- ✅ **Adds `ScrollNode` layout modifier**
- ✅ **Measures child with infinite constraints**
- ✅ **Reads `state.value` and applies offset**
- ✅ **Clips content to viewport**
- ✅ Overscroll rendering

**Implementation:**
```kotlin
fun Modifier.verticalScroll(state: ScrollState, ...): Modifier =
    scroll(state, isVertical = true, ...)

private fun Modifier.scroll(...): Modifier {
    val scrollableArea = scrollableArea(state, orientation, ...)  // scrollable + clip + overscroll
    return scrollableArea.then(ScrollingLayoutElement(state, ...))  // + ScrollNode
}
```

**Expanded Chain:**
```kotlin
Modifier
    .scrollable(state, orientation, ...)  // Input handling
    .clipScrollableArea(...)               // Clipping
    .overscroll(overscrollEffect, ...)     // Overscroll rendering
    .then(ScrollNode(...))                  // Layout offset
```

**When to use:**
- Standard scrolling behavior
- Simple scrollable containers
- Want automatic content offset

**Example:**
```kotlin
Column(
    modifier = Modifier
        .size(300.dp)
        .verticalScroll(rememberScrollState())
) {
    // Content automatically scrolls
}
```

---

## Key Classes Reference

| Class | Location | Role |
|-------|----------|------|
| `PointerInputEventProcessor` | ui/input/pointer | Entry point, hit testing |
| `HitPathTracker` | ui/input/pointer | Tree manager, 3-pass dispatcher |
| `Node` (inner) | ui/input/pointer | Per-modifier node, caching, transforms |
| `PointerEvent` | ui/input/pointer | Event container |
| `PointerInputChange` | ui/input/pointer | Single pointer change, consumption |
| `SuspendingPointerInputModifierNode` | ui/input/pointer | Suspending pointer input, `awaitPointerEventScope` |
| `ScrollableNode` | foundation/gestures | Input handler, delegates to `ScrollingLogic` |
| `DragGestureNode` | foundation/gestures | Drag detection via pointer input |
| `ScrollingLogic` | foundation/gestures | Nested scroll, overscroll, calls `scrollBy()` |
| `ScrollState` | foundation | **State holder, updates `value` on scroll** |
| `ScrollNode` | foundation | **LayoutModifierNode, reads `value`, applies offset** |
| `NestedScrollDispatcher` | ui/input/nestedscroll | Nested scroll pre/post coordination |

---

## Critical Timing: When Does What Happen?

### During Input Event (Synchronous - Same Frame)
1. Touch event arrives
2. Hit testing (if needed)
3. Three-pass dispatch to modifiers
4. Drag gesture detected
5. `ScrollingLogic.scrollByWithOverscroll()` called
6. `ScrollState.value` **updated**
7. Event consumption marked
8. Event processing complete

### After State Update (Asynchronous - Next Frame)
1. Snapshot system detects state write
2. Recomposition scheduled
3. **Next frame starts**
4. Recomposition (if needed)
5. Layout pass runs
6. `ScrollNode.measure()` called
7. `state.value` **read**
8. Offset calculated
9. `placeable.placeRelativeWithLayer()` **content moved**
10. Draw pass renders at new position
11. **User sees scroll**

**Gap:** State updates in Frame N, visual update happens in Frame N+1

---

## Overscroll and Nested Scroll

### Overscroll

**Mechanism:**
```kotlin
overscrollEffect.applyToScroll(delta, source) { actualDelta ->
    performScroll(actualDelta, source)
}
```

**Flow:**
1. Overscroll effect receives delta first
2. If at boundary and scrolling past, applies overscroll visual
3. Calls `performScroll()` with adjusted delta
4. Consumes what can be scrolled normally
5. Leftover handled by overscroll effect

**Example:** Scroll at top, drag down 100px
- Scroll can't consume (at limit 0)
- Overscroll shows stretch effect for 100px
- On release, stretch animates back

---

### Nested Scroll

**Pre-Scroll (Parent First):**
```kotlin
val preConsumed = parentScrollable.dispatchPreScroll(delta)
val available = delta - preConsumed
// Child uses available
```

**Post-Scroll (Parent Handles Leftovers):**
```kotlin
val leftover = delta - selfConsumed
val postConsumed = parentScrollable.dispatchPostScroll(selfConsumed, leftover)
```

**Example:** NestedScrollView
- Parent scrollable container
- Child scrollable list

**Scroll down at top of child:**
1. Child pre-scroll: passes to parent
2. Parent consumes (scrolls itself)
3. Child gets leftover (if any)
4. Child scrolls what it can
5. Child post-scroll: passes leftover to parent

**Result:** Parent scrolls first, then child

---

## Advanced: Fling Animation

```kotlin
suspend fun onScrollStopped(velocity: Velocity) {
    // Pre-fling
    val preConsumed = nestedScrollDispatcher.dispatchPreFling(velocity)
    val available = velocity - preConsumed
    
    // Self fling
    val velocityLeft = doFlingAnimation(available)
    
    // Post-fling
    nestedScrollDispatcher.dispatchPostFling(available - velocityLeft, velocityLeft)
}

suspend fun doFlingAnimation(velocity: Velocity): Velocity {
    scroll(MutatePriority.Default) {
        with(flingBehavior) {
            performFling(velocity.toFloat())
        }
    }
}

// DefaultFlingBehavior:
override suspend fun ScrollScope.performFling(initialVelocity: Float): Float {
    animationState.animateDecay(flingDecay) {
        val delta = value - lastValue
        val consumed = scrollBy(delta)  // <-- Calls scrollBy many times during animation
        lastValue = value
        velocityLeft = this.velocity
        if (abs(delta - consumed) > 0.5f) cancelAnimation()  // Hit boundary
    }
    return velocityLeft
}
```

**Key Points:**
- Fling is animated `scrollBy()` calls
- Uses decay animation spec (spline-based by default)
- Each animation frame calls `scrollBy(delta)`
- Stops when hitting boundary (unconsumed delta)
- Leftover velocity bubbles to nested scroll parent

---

## Answers to Common Questions

### Q: Why doesn't `Modifier.scrollable()` move content?

**A:** By design. `scrollable()` is a **low-level input modifier**. It:
- Only handles input events
- Updates state
- Allows custom layout behavior

Use `verticalScroll()` / `horizontalScroll()` for standard scrolling.

---

### Q: When exactly does content move?

**A:** In the **layout pass** of the **frame after** state update:
1. Frame N: State updated during event
2. Frame N+1: Layout reads state, applies offset

---

### Q: How does clipping work?

**A:** `scrollableArea()` includes `Modifier.clipScrollableArea()`:
```kotlin
fun Modifier.clipScrollableArea(...): Modifier =
    this.then(ClipScrollableAreaElement(...))

// During draw:
clipRect(0, 0, size.width, size.height) {
    // Draw content (already offset by ScrollNode)
}
```

---

### Q: Can I observe scroll events?

**A:** Yes, multiple ways:

**1. Read state directly:**
```kotlin
val scrollState = rememberScrollState()
LaunchedEffect(scrollState.value) {
    println("Scrolled to: ${scrollState.value}")
}
```

**2. Use `InteractionSource`:**
```kotlin
val scrollState = rememberScrollState()
// scrollState.interactionSource emits drag events
```

**3. Custom `ScrollableState`:**
```kotlin
val customState = remember {
    ScrollableState { delta ->
        // Custom logic
        println("Scroll delta: $delta")
        delta  // Consume all
    }
}
```

---

## Comparison Table: Jetpack Compose vs What Rust Implementation Needs

| Feature | Jetpack Compose | Rust Implementation Needs |
|---------|-----------------|---------------------------|
| **Event Entry** | `PointerInputEventProcessor` | Platform event receiver |
| **Hit Testing** | `LayoutNode.hitTest()` | Hit test on layout tree |
| **Event Dispatch** | 3-pass (Initial/Main/Final) | Multi-pass system |
| **Position Transform** | Per-node local coordinates | Transform to each modifier's space |
| **Consumption** | Shared via `consumedDelegate` | Shared consumption state |
| **Gesture Detection** | `awaitPointerEventScope` | Async gesture APIs |
| **Scroll State** | `ScrollState` with snapshot | Observable state |
| **Layout Offset** | `LayoutModifierNode.measure()` | Layout modifier reads state |
| **Placement** | `placeRelativeWithLayer()` | Offset during placement |
| **Nested Scroll** | Pre/post scroll coordination | Nested scroll protocol |
| **Overscroll** | `OverscrollEffect` wrapper | Visual overscroll effects |
| **Fling** | Decay animation | Velocity-based animation |

---

## Next Steps: Part 2

Investigate the current Rust implementation to determine:

1. ✅ How are pointer events received and dispatched?
2. ✅ Is there a hit testing mechanism?
3. ✅ How many event passes exist?
4. ✅ How is coordinate transformation handled?
5. ✅ How does event consumption work?
6. ✅ How do modifiers receive events?
7. ✅ Is there a state + layout offset pattern?
8. ✅ What's the current scroll implementation?
9. ❌ What's missing for proper scrollable support?
10. ❌ What architecture changes are needed?
