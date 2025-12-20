//! Observable state holder for text field content.
//!
//! Matches Jetpack Compose's `TextFieldState` from
//! `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/input/TextFieldState.kt`.

use super::{TextFieldBuffer, TextRange};
use compose_core::MutableState;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

/// Immutable snapshot of text field content.
///
/// This represents the text, selection, and composition state at a point in time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextFieldValue {
    /// The text content
    pub text: String,
    /// Current selection or cursor position
    pub selection: TextRange,
    /// IME composition range, if any
    pub composition: Option<TextRange>,
}

impl TextFieldValue {
    /// Creates a new value with the given text and cursor at end.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let len = text.len();
        Self {
            text,
            selection: TextRange::cursor(len),
            composition: None,
        }
    }

    /// Creates a value with specified text and selection.
    pub fn with_selection(text: impl Into<String>, selection: TextRange) -> Self {
        let text = text.into();
        let selection = selection.coerce_in(text.len());
        Self {
            text,
            selection,
            composition: None,
        }
    }
}

type ChangeListener = Box<dyn Fn(&TextFieldValue)>;

/// Maximum capacity for undo stack
const UNDO_CAPACITY: usize = 100;

/// Timeout for undo coalescing in milliseconds.
/// Consecutive edits within this window are grouped into a single undo.
const UNDO_COALESCE_MS: u128 = 1000;

/// Inner state for TextFieldState - contains editing machinery ONLY.
/// Value storage is handled by MutableState.
pub struct TextFieldStateInner {
    /// Flag to prevent concurrent edits  
    is_editing: bool,
    /// Listeners to notify on changes
    listeners: Vec<ChangeListener>,
    /// Undo stack - previous states to restore
    undo_stack: VecDeque<TextFieldValue>,
    /// Redo stack - states undone that can be redone
    redo_stack: VecDeque<TextFieldValue>,
    /// Desired column for up/down navigation (preserved between vertical moves)
    desired_column: Cell<Option<usize>>,
    /// Last edit timestamp for undo coalescing
    last_edit_time: Cell<Option<instant::Instant>>,
    /// Snapshot before the current coalescing group started
    /// Only pushed to undo_stack when coalescing breaks
    pending_undo_snapshot: RefCell<Option<TextFieldValue>>,
    /// Cached line start offsets for O(1) line lookups during rendering.
    /// Invalidated on text change. Each entry is byte offset of line start.
    /// e.g., for "ab\ncd" -> [0, 3] (line 0 starts at 0, line 1 starts at 3)
    line_offsets_cache: RefCell<Option<Vec<usize>>>,
}

/// RAII guard for is_editing flag - ensures panic safety
struct EditGuard<'a> {
    inner: &'a RefCell<TextFieldStateInner>,
}

impl<'a> EditGuard<'a> {
    fn new(inner: &'a RefCell<TextFieldStateInner>) -> Result<Self, ()> {
        {
            let borrowed = inner.borrow();
            if borrowed.is_editing {
                return Err(());  // Already editing
            }
        }
        inner.borrow_mut().is_editing = true;
        Ok(Self { inner })
    }
}

impl Drop for EditGuard<'_> {
    fn drop(&mut self) {
        self.inner.borrow_mut().is_editing = false;
    }
}

/// Observable state holder for text field content.
///
/// This is the primary API for managing text field state. All edits go through
/// the [`edit`](Self::edit) method which provides a mutable buffer.
///
/// # Example
///
/// ```ignore
/// use compose_foundation::text::TextFieldState;
///
/// let state = TextFieldState::new("Hello");
///
/// // Edit the text
/// state.edit(|buffer| {
///     buffer.place_cursor_at_end();
///     buffer.insert(", World!");
/// });
///
/// assert_eq!(state.text(), "Hello, World!");
/// ```
///
/// # Thread Safety
///
/// `TextFieldState` uses `Rc<RefCell<...>>` internally and is not thread-safe.
/// It should only be used from the main thread.
#[derive(Clone)]
pub struct TextFieldState {
    /// Internal state for editing machinery.
    /// Public for cross-crate pointer-based identity comparison (Hash).
    pub inner: Rc<RefCell<TextFieldStateInner>>,
    
    /// Value storage - the SINGLE source of truth for text field value.
    /// Uses MutableState for reactive composition integration.
    value: Rc<MutableState<TextFieldValue>>,
}

impl std::fmt::Debug for TextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.with(|v| {
            f.debug_struct("TextFieldState")
                .field("text", &v.text)
                .field("selection", &v.selection)
                .finish()
        })
    }
}

impl TextFieldState {
    /// Creates a new text field state with the given initial text.
    pub fn new(initial_text: impl Into<String>) -> Self {
        let initial_value = TextFieldValue::new(initial_text);
        Self {
            inner: Rc::new(RefCell::new(TextFieldStateInner {
                is_editing: false,
                listeners: Vec::new(),
                undo_stack: VecDeque::new(),
                redo_stack: VecDeque::new(),
                desired_column: Cell::new(None),
                last_edit_time: Cell::new(None),
                pending_undo_snapshot: RefCell::new(None),
                line_offsets_cache: RefCell::new(None),
            })),
            value: Rc::new(compose_core::mutableStateOf(initial_value)),
        }
    }

    /// Creates a state with initial text and selection.
    pub fn with_selection(initial_text: impl Into<String>, selection: TextRange) -> Self {
        let initial_value = TextFieldValue::with_selection(initial_text, selection);
        Self {
            inner: Rc::new(RefCell::new(TextFieldStateInner {
                is_editing: false,
                listeners: Vec::new(),
                undo_stack: VecDeque::new(),
                redo_stack: VecDeque::new(),
                desired_column: Cell::new(None),
                last_edit_time: Cell::new(None),
                pending_undo_snapshot: RefCell::new(None),
                line_offsets_cache: RefCell::new(None),
            })),
            value: Rc::new(compose_core::mutableStateOf(initial_value)),
        }
    }

    /// Gets the desired column for up/down navigation.
    pub fn desired_column(&self) -> Option<usize> {
        self.inner.borrow().desired_column.get()
    }
    
    /// Sets the desired column for up/down navigation.
    pub fn set_desired_column(&self, col: Option<usize>) {
        self.inner.borrow().desired_column.set(col);
    }

    /// Returns the current text content.
    /// Creates composition dependency when read during composition.
    pub fn text(&self) -> String {
        self.value.with(|v| v.text.clone())
    }

    /// Returns the current selection range.
    pub fn selection(&self) -> TextRange {
        self.value.with(|v| v.selection)
    }

    /// Returns the current composition (IME) range, if any.
    pub fn composition(&self) -> Option<TextRange> {
        self.value.with(|v| v.composition)
    }

    /// Returns cached line start offsets for efficient multiline operations.
    ///
    /// Each entry is the byte offset where a line starts. For example:
    /// - "ab\ncd" -> [0, 3] (line 0 starts at 0, line 1 starts at 3)
    /// - "" -> [0]
    ///
    /// The cache is lazily computed on first access and invalidated on text change.
    /// This avoids O(n) string splitting on every frame during selection rendering.
    pub fn line_offsets(&self) -> Vec<usize> {
        let inner = self.inner.borrow();
        
        // Check if cached
        if let Some(ref offsets) = *inner.line_offsets_cache.borrow() {
            return offsets.clone();
        }
        
        // Compute line offsets
        let text = self.text();
        let mut offsets = vec![0];
        for (i, c) in text.char_indices() {
            if c == '\n' {
                // Next line starts at i + 1 (byte after newline)
                // Note: '\n' is always 1 byte in UTF-8
                offsets.push(i + 1);
            }
        }
        
        // Cache and return
        *inner.line_offsets_cache.borrow_mut() = Some(offsets.clone());
        offsets
    }

    /// Invalidates the cached line offsets. Called internally on text change.
    fn invalidate_line_cache(&self) {
        self.inner.borrow().line_offsets_cache.borrow_mut().take();
    }
    
    /// Copies the selected text without modifying the clipboard.
    /// Returns the selected text, or None if no selection.
    pub fn copy_selection(&self) -> Option<String> {
        self.value.with(|v| {
            let selection = v.selection;
            if selection.collapsed() {
                return None;
            }
            let start = selection.min();
            let end = selection.max();
            Some(v.text[start..end].to_string())
        })
    }

    /// Returns the current value snapshot.
    /// Creates composition dependency when read during composition.
    pub fn value(&self) -> TextFieldValue {
        self.value.with(|v| v.clone())
    }

    /// Adds a listener that is called when the value changes.
    ///
    /// Returns the listener index for removal.
    pub fn add_listener(&self, listener: impl Fn(&TextFieldValue) + 'static) -> usize {
        let mut inner = self.inner.borrow_mut();
        let index = inner.listeners.len();
        inner.listeners.push(Box::new(listener));
        index
    }

    /// Sets the selection directly without going through undo stack.
    /// Use this for transient selection changes like during drag selection.
    pub fn set_selection(&self, selection: TextRange) {
        let new_value = self.value.with(|v| {
            let len = v.text.len();
            TextFieldValue {
                text: v.text.clone(),
                selection: selection.coerce_in(len),
                composition: v.composition,
            }
        });
        self.value.set(new_value);
    }

    /// Returns true if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.inner.borrow().undo_stack.is_empty()
    }

    /// Returns true if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.inner.borrow().redo_stack.is_empty()
    }

    /// Undoes the last edit.
    /// Returns true if undo was performed.
    pub fn undo(&self) -> bool {
        // First, flush any pending coalescing snapshot so it becomes the undo target
        self.flush_undo_group();
        
        let mut inner = self.inner.borrow_mut();
        if let Some(previous_state) = inner.undo_stack.pop_back() {
            // Save current state to redo stack
            let current = self.value.with(|v| v.clone());
            inner.redo_stack.push_back(current);
            // Clear coalescing state since we're undoing
            inner.last_edit_time.set(None);
            drop(inner);
            // Update value via MutableState (triggers recomposition)
            self.value.set(previous_state);
            true
        } else {
            false
        }
    }

    /// Redoes the last undone edit.
    /// Returns true if redo was performed.
    pub fn redo(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        if let Some(redo_state) = inner.redo_stack.pop_back() {
            // Save current state to undo stack
            let current = self.value.with(|v| v.clone());
            inner.undo_stack.push_back(current);
            drop(inner);
            // Update value via MutableState (triggers recomposition)
            self.value.set(redo_state);
            true
        } else {
            false
        }
    }

    /// Edits the text field content.
    ///
    /// The provided closure receives a mutable buffer that can be used to
    /// modify the text and selection. After the closure returns, the changes
    /// are committed and listeners are notified.
    ///
    /// # Undo Coalescing
    ///
    /// Consecutive character insertions within the coalescing timeout are grouped
    /// into a single undo entry. The group breaks when:
    /// - Timeout expires (1 second between edits)
    /// - Whitespace or newline is typed
    /// - Cursor position jumps (non-consecutive insert)
    /// - A non-insert operation occurs (delete, paste multi-char, etc.)
    ///
    /// # Panics
    ///
    /// Panics if called while already editing (no concurrent or nested edits).
    pub fn edit<F>(&self, f: F)
    where
        F: FnOnce(&mut TextFieldBuffer),
    {
        // RAII guard ensures is_editing is cleared even on panic
        let _guard = EditGuard::new(&self.inner)
            .expect("TextFieldState does not support concurrent or nested editing");

        // Create buffer from current value
        let current = self.value();
        let mut buffer = TextFieldBuffer::with_selection(&current.text, current.selection);
        if let Some(comp) = current.composition {
            buffer.set_composition(Some(comp));
        }

        // Execute the edit
        f(&mut buffer);

        // Build new value
        let new_value = TextFieldValue {
            text: buffer.text().to_string(),
            selection: buffer.selection(),
            composition: buffer.composition(),
        };

        // Only update and notify if changed
        let changed = new_value != current;
        let text_changed = new_value.text != current.text;
        
        // Invalidate line cache if text changed (not just selection)
        if text_changed {
            self.invalidate_line_cache();
        }
        
        if changed {
            let now = instant::Instant::now();
            
            // Determine if we should break the undo coalescing group
            let should_break_group = {
                let inner = self.inner.borrow();
                
                // Check timeout
                let timeout_expired = inner.last_edit_time.get()
                    .map(|last| now.duration_since(last).as_millis() > UNDO_COALESCE_MS)
                    .unwrap_or(true);
                
                if timeout_expired {
                    true
                } else {
                    // Check if this looks like a single character insert
                    let text_delta = new_value.text.len() as i64 - current.text.len() as i64;
                    let is_single_char_insert = text_delta == 1;
                    
                    // Check if it's whitespace/newline (break group on word boundaries)
                    let ends_with_whitespace = new_value.text.ends_with(char::is_whitespace);
                    
                    // Check if cursor jumped (non-consecutive editing)
                    let cursor_jumped = new_value.selection.start != current.selection.start + 1
                        && new_value.selection.start != current.selection.end + 1;
                    
                    // Break if not a simple character insert, or if whitespace/newline, or cursor jumped
                    !is_single_char_insert || ends_with_whitespace || cursor_jumped
                }
            };
            
            {
                let inner = self.inner.borrow();
                
                if should_break_group {
                    // Push pending snapshot (if any) to undo stack, then start new group
                    let pending = inner.pending_undo_snapshot.take();
                    drop(inner);
                    
                    let mut inner = self.inner.borrow_mut();
                    if let Some(snapshot) = pending {
                        if inner.undo_stack.len() >= UNDO_CAPACITY {
                            inner.undo_stack.pop_front();
                        }
                        inner.undo_stack.push_back(snapshot);
                    }
                    // Clear redo stack on new edit
                    inner.redo_stack.clear();
                    // Start new coalescing group with current state as pending snapshot
                    drop(inner);
                    self.inner.borrow().pending_undo_snapshot.replace(Some(current.clone()));
                } else {
                    // Continue coalescing - pending snapshot stays as-is
                    // If no pending snapshot, start one
                    if inner.pending_undo_snapshot.borrow().is_none() {
                        inner.pending_undo_snapshot.replace(Some(current.clone()));
                    }
                    drop(inner);
                    // Clear redo stack on new edit
                    self.inner.borrow_mut().redo_stack.clear();
                }
                
                // Update last edit time
                self.inner.borrow().last_edit_time.set(Some(now));
            }
            
            // Update value via MutableState (triggers recomposition)
            self.value.set(new_value.clone());
        }
        
        // Explicitly drop guard to clear is_editing BEFORE notifying listeners
        // This ensures listeners see clean state and can start new edits if needed
        drop(_guard);

        // Notify listeners outside of borrow
        if changed {
            let listener_count = self.inner.borrow().listeners.len();
            for i in 0..listener_count {
                let inner = self.inner.borrow();
                if i < inner.listeners.len() {
                    (inner.listeners[i])(&new_value);
                }
            }
        }
    }
    
    /// Flushes any pending undo snapshot to the undo stack.
    /// Call this when a coalescing break is desired (e.g., focus lost).
    pub fn flush_undo_group(&self) {
        let inner = self.inner.borrow();
        if let Some(snapshot) = inner.pending_undo_snapshot.take() {
            drop(inner);
            let mut inner = self.inner.borrow_mut();
            if inner.undo_stack.len() >= UNDO_CAPACITY {
                inner.undo_stack.pop_front();
            }
            inner.undo_stack.push_back(snapshot);
        }
    }

    /// Sets the text and places cursor at end.
    pub fn set_text(&self, text: impl Into<String>) {
        let text = text.into();
        self.edit(|buffer| {
            buffer.clear();
            buffer.insert(&text);
        });
    }

    /// Sets the text and selects all.
    pub fn set_text_and_select_all(&self, text: impl Into<String>) {
        let text = text.into();
        self.edit(|buffer| {
            buffer.clear();
            buffer.insert(&text);
            buffer.select_all();
        });
    }
}

impl Default for TextFieldState {
    fn default() -> Self {
        Self::new("")
    }
}

impl PartialEq for TextFieldState {
    fn eq(&self, other: &Self) -> bool {
        // Compare by Rc pointer identity - same state instance
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_core::{DefaultScheduler, Runtime};
    use std::sync::Arc;

    /// Sets up a test runtime and keeps it alive for the duration of the test.
    /// This is required because TextFieldState uses MutableState which requires
    /// an active runtime context.
    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    #[test]
    fn new_state_has_cursor_at_end() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            assert_eq!(state.text(), "Hello");
            assert_eq!(state.selection(), TextRange::cursor(5));
        });
    }

    #[test]
    fn edit_updates_text() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            state.edit(|buffer| {
                buffer.place_cursor_at_end();
                buffer.insert(", World!");
            });
            assert_eq!(state.text(), "Hello, World!");
        });
    }

    #[test]
    fn edit_updates_selection() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            state.edit(|buffer| {
                buffer.select_all();
            });
            assert_eq!(state.selection(), TextRange::new(0, 5));
        });
    }

    #[test]
    fn set_text_replaces_content() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            state.set_text("Goodbye");
            assert_eq!(state.text(), "Goodbye");
            assert_eq!(state.selection(), TextRange::cursor(7));
        });
    }

    #[test]
    #[should_panic(expected = "concurrent or nested editing")]
    fn nested_edit_panics() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            let state_clone = state.clone();
            state.edit(move |_buffer| {
                state_clone.edit(|_| {}); // This should panic
            });
        });
    }

    #[test]
    fn listener_is_called_on_change() {
        with_test_runtime(|| {
            use std::cell::Cell;
            use std::rc::Rc;

            let state = TextFieldState::new("Hello");
            let called = Rc::new(Cell::new(false));
            let called_clone = called.clone();
            
            state.add_listener(move |_value| {
                called_clone.set(true);
            });

            state.edit(|buffer| {
                buffer.insert("!");
            });

            assert!(called.get());
        });
    }
}

