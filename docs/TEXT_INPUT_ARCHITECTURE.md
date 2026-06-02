# Text Input System Architecture

> **Definitive Guide** to the Cranpose text input subsystem.
> This document describes the *current state* of the architecture, data flows, and specific implementation quirks.
>
> **Last Updated**: 2025-12-20 (post code review + remedy fixes)

---

## 1. System Overview

The text input system is a vertical slice covering data storage, rendering, and input processing. It follows the **Jetpack Compose** architecture but is adapted for Rust's ownership model and the `cranpose-ui` rendering pipeline.

### Core Component Interactive Flow

1.  **Input**: User presses a key -> `winit` -> `AppShell` -> `FocusManager` -> `TextFieldHandler`.
2.  **State**: Handler mutates `TextFieldState` (via `edit()` closure).
3.  **Reaction**: `TextFieldState` (backed by `MutableState`) triggers recomposition.
4.  **Layout**: `TextFieldModifierNode` measures text using cached `TextMeasurer`.
5.  **Draw**: `TextFieldModifierNode` draws text + selection + cursor (if focused).

---

## 2. Data Models (`cranpose-foundation`)

### `TextFieldState` (The Source of Truth)
*   **Location**: `crates/cranpose-foundation/src/text/state.rs`
*   **Role**: Internal state holder. Application logic holds this and passes it to the widget.
*   **Storage**: Internally wraps `Rc<RefCell<TextFieldStateInner>>` and `Rc<MutableState<TextFieldValue>>`.
*   **Quirk**: Changes *must* go through `.edit(|buffer| ...)` closure. This ensures change notification and undo state capture.

### `TextFieldBuffer` (The Editor)
*   **Location**: `crates/cranpose-foundation/src/text/buffer.rs`
*   **Role**: Temporary, mutable view of the text/selection used *only* inside an `edit` block.
*   **Quirk**: All indices are **UTF-8 byte offsets**, not character indices. It enforces valid Unicode boundaries for all operations.

### `TextRange` (The Cursor/Selection)
*   **Location**: `crates/cranpose-foundation/src/text/range.rs`
*   **Role**: Immutable struct `{ start: usize, end: usize }`.
*   **Quirks**:
    *   `start` can be greater than `end` (indicating reverse selection direction). Always use `.min()`/`.max()` for slicing.
    *   `safe_slice(&str)` method handles UTF-8 boundary clamping automatically, avoiding panics on invalid byte indices.

---

## 3. Input & Focus Subsystem (`cranpose-ui`)

### The O(1) Focus Dispatch Trick
*   **Problem**: finding the focused node in a deep UI tree is O(N).
*   **Solution**: We use **Thread-Local Storage** to store the currently focused handler.
*   **Implementation**: `crates/cranpose-ui/src/text_field_focus.rs`
    *   `thread_local! { static FOCUSED_HANDLER: ... }`
*   **Flow**:
    1.  `AppShell` receives KeyDown.
    2.  Calls `text_field_focus::dispatch_key_event(e)`.
    3.  Focus module calls `.handle_key(e)` on the stored `Rc<dyn FocusedTextFieldHandler>` directly.
    4.  **Zero tree traversal required.**

### Key Event Handling
*   **Location**: `handle_key_event_impl` in `text_field_input.rs`.
*   **Role**: Shared logic for processing `KeyEvent` -> `TextFieldState` mutations.
*   **Scope**: Handles standard editing (typing, backspace, delete, enter) and navigation (arrows, home/end, ctrl+arrows).
*   **Quirk**: Word boundary detection (`word_boundaries.rs`) uses extensive Unicode classification to properly jump words.

---

## 4. Rendering Pipeline (`cranpose-ui`)

### `TextFieldModifierNode`
*   **Location**: `crates/cranpose-ui/src/text_field_modifier_node.rs`
*   **Role**: The "Node" that lives in the UI tree. Handles Layout, Draw, and Pointer Input.
*   **Architecture**: It is a *Modifier Node*, not a basic Widget. This separates layout policy (where to place it) from the text logic itself.

### The Draw Closure Pattern
To avoid ownership issues during the draw phase, `create_draw_closure(self)` captures necessary state (Rc references) and returns a `Fn(Size) -> Vec<DrawPrimitive>`.
*   **Quirk**: Focus and Cursor Visibility are checked **at draw time**, inside the closure.
    *   This means gaining focus *does not need a layout pass*, only a repaint.
    *   Cursor blinking *does not need a layout pass*, only a repaint.

### Pointer Input Delegation
*   **Pattern**: Following Jetpack Compose's `TextFieldDecoratorModifier`, `on_pointer_event()` is a no-op.
*   **All logic** is in the `pointer_input_handler()` closure, enabling clean separation.
*   **Features**: Click focus, cursor positioning, double-click word selection, triple-click select-all, drag selection.

### Cursor Animation
*   **Location**: `crates/cranpose-ui/src/cursor_animation.rs`
*   **Mechanism**: Global thread-local state tracks "is cursor visible".
*   **Loop**: `AppShell` event loop uses `ControlFlow::WaitUntil` to wake up exactly when the cursor needs to toggle (every 500ms).
*   **Optimization**: If the text field is not focused, the timer stops completely.

---

## 5. Specific Implementation Quirks

### 1. Undo Coalescing
Users hate when `Ctrl+Z` undoes one character at a time.
*   **Algorithm**: Consecutive edits are grouped into a "Batch".
*   **Batch Breaks When**:
    1.  Time since last edit > `1000ms`.
    2.  User types whitespace or newline (word break).
    3.  Selection moves explicitly (click/arrow key).
    4.  Non-insert operation occurs (delete, paste).
*   **Implementation**: `TextFieldState::edit()` tracks `last_edit_time` and manages a `pending_undo_snapshot`. The snapshot is only "committed" to the stack when the batch breaks. Nested edit attempts return `false` and leave the active edit unchanged.

### 2. Platform Clipboard
*   **Desktop**: Uses `arboard` crate. Copied text is sent to OS clipboard synchronously.
*   **Web**: (Planned) `navigator.clipboard`.
*   **Middle-Click (Linux)**: Supported via `Primary` clipboard selection. Selection changes in `TextField` automatically update the Primary buffer.

### 3. Text Measurement (Current Limitation)
*   **Status**: Currently uses `MonospacedTextMeasurer`.
*   **Metrics**: Hardcoded 20px line height, 8px char width.
*   **Implication**: Text inputs look monospaced regardless of font settings until a real text engine (Skia/Cosmic-Text) is integrated.

### 4. Layout Invalidation
*   **Scoped**: When text changes, we call `request_layout_invalidation()` on the specific node ID.
*   **Optimization**: This prevents the entire UI tree from re-measuring. Only the text field and its parents re-measure.

### 5. Line Offset Caching
*   **Location**: `TextFieldStateInner.line_offsets_cache` in `state.rs`
*   **Role**: Lazy-computed `Vec<usize>` of byte offsets where each line starts.
*   **Invalidation**: Cache is cleared on any text change via `edit()`.
*   **Benefit**: Enables O(1) line lookups for multiline rendering instead of per-frame string splitting.

---

## 6. File Map

| File Path | Component | Responsibility |
|-----------|-----------|----------------|
| `cranpose-foundation/text/state.rs` | **State** | Data holder, Undo/Redo, Line cache. |
| `cranpose-foundation/text/buffer.rs` | **Buffer** | Mutable editing logic, Unicode safety. |
| `cranpose-foundation/text/range.rs` | **Range** | Selection/cursor, `safe_slice()` utility. |
| `cranpose-ui/widgets/basic_text_field.rs` | **Widget**| Composable entry point. |
| `cranpose-ui/text_field_modifier_node.rs` | **Node** | Layout, Draw, Pointer (delegated). |
| `cranpose-ui/text_field_focus.rs` | **Focus** | O(1) dispatch mechanism. |
| `cranpose-ui/text_field_handler.rs` | **Bridge** | Connects Focus system to State. |
| `cranpose-ui/text_field_input.rs` | **Input** | Shared keyboard event handling. |
| `cranpose-ui/word_boundaries.rs` | **Text** | Unicode word boundary detection. |
| `cranpose-ui/cursor_animation.rs` | **Anim** | Blink timer logic. |
| `cranpose/desktop.rs` | **Platform**| `winit` key mapping table. |

---

## 7. Feature Parity Status

| Feature | Status | Notes |
|---------|--------|-------|
| `TextFieldState` | ✅ | Basic parity |
| `TextFieldBuffer` | ✅ | Basic parity |
| `TextRange` | ✅ | Full parity + `safe_slice()` |
| Undo/Redo | ✅ | With coalescing |
| Cursor Blink | ✅ | Timer-based |
| Focus O(1) Dispatch | ✅ | Thread-local |
| Keyboard Input | ✅ | Standard keys |
| Clipboard | ✅ | Desktop only |
| Selection (click/drag) | ✅ | Delegated pointer handler |
| Word Selection (dbl-click) | ✅ | Unicode-aware |
| Single Line Mode | ✅ | `TextFieldLineLimits::SingleLine` |
| IME / Composition | ✅ | Preedit + underline + commit |
| Line Offset Cache | ✅ | Lazy computed, invalidated on edit |
| `InputTransformation` | ❌ | Validation filters |
| `OutputTransformation` | ❌ | Visual-only transforms |
| `TextFieldDecorator` | ❌ | Wrapper composable |
| `KeyboardOptions` | ❌ | IME hints |
| `KeyboardActions` | ❌ | Enter key handler |
| Selection Handles | ❌ | Touch handles |
| Context Menu | ❌ | Cut/Copy/Paste menu |
| `BasicSecureTextField` | ❌ | Password field |
| Real Text Layout | ❌ | Using monospace fallback |

---

## 8. Roadmap

### 🔴 P0: Critical Priority

**Real Text Layout Engine** (10 days)
- Integrate `cosmic-text` or Skia
- Variable-width fonts, complex scripts, emoji

---

### 🟠 P1: Core Feature Parity

**Keyboard Options & Actions** (2 days)
- `KeyboardOptions` struct (capitalization, keyboardType, imeAction)
- `on_submit` callback for Enter key

**Context Menu** (2 days)
- Right-click Cut/Copy/Paste

**Input Transformation** (3 days)
- Trait for chainable input filters (run after edit, before commit)
- Built-ins: `MaxLength`, `AllCaps`, `Digits`

---

### 🟡 P2: UX Polish

**Decorator** (1 day) - Wrapper composable for icons/labels

**Secure Text Field** (1 day) - Password obfuscation (`•••••`)

**Output Transformation** (2 days) - Visual-only transforms (e.g., credit card formatting)

**Selection Handles** (5 days) - Touch handles + magnifier

---

### 🔵 P3: Advanced Features

- **AnnotatedString** - Rich text with inline styles
- **Inline Content** - Embed composables in text
- **Text Links** - Clickable URL spans

---

### ⚪ P4: Extreme Performance - 10GB Support

**Viewport-Only Rendering** (5 days)
- Track scroll position in lines
- Layout/draw only visible lines
- Virtual scrolling integration

**Rope Data Structure** (10 days)
- Replace `String` with `ropey` crate
- O(log n) insert/delete at arbitrary positions
- O(log n) line indexing

**Async Loading** (3 days)
- Stream large files in chunks
- Progressive line cache building

---

### Priority Order (Implementation-Focused)

**Rationale**: Text engine first (unblocks everything), then quick wins, then features, then scale.

| Priority | Item | Effort | Why |
|----------|------|--------|-----|
| 🔴 P0 | Real text layout engine | 10 days | Unblocks all features - monospace is broken foundation |
| 🟠 P1 | Keyboard options | 2 days | Core feature parity |
| 🟠 P1 | Context menu | 2 days | Expected UX |
| 🟠 P1 | Input transformation | 3 days | Core feature parity |
| 🟡 P2 | Decorator | 1 day | Polish |
| 🟡 P2 | Secure text field | 1 day | Polish |
| 🟡 P2 | Output transformation | 2 days | Polish |
| 🔵 P3 | AnnotatedString | TBD | After text engine works |
| 🔵 P3 | Inline Content | TBD | After text engine works |
| ⚪ P4 | Viewport-only rendering | 5 days | Only for 10GB scale |
| ⚪ P4 | Rope data structure | 10 days | Only for 10GB scale |

**Total remaining: ~44 days**
