use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{MutableState, RuntimeCell};

use super::{InputTransformation, TextFieldBuffer, TextRange};

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

const UNDO_CAPACITY: usize = 100;

const UNDO_COALESCE_MS: u128 = 1000;

/// Spare room in an edit's copy of the text, so a typed character does not
/// grow the copy and move the whole text a second time.
const EDIT_HEADROOM_BYTES: usize = 64;

/// What an edit changed, against the value before it.
struct EditChange {
    text_changed: bool,
    /// A one-byte insert right after the cursor that does not end in
    /// whitespace: typing that joins the pending undo group.
    continues_typing: bool,
}

impl EditChange {
    fn between(before: &TextFieldValue, after: &TextFieldBuffer) -> Option<Self> {
        let selection = after.selection();
        let text_changed = after.text() != before.text;
        if !text_changed
            && selection == before.selection
            && after.composition() == before.composition
        {
            return None;
        }
        let single_byte_insert = after.len() == before.text.len() + 1;
        let cursor_followed = selection.start == before.selection.start + 1
            || selection.start == before.selection.end + 1;
        Some(Self {
            text_changed,
            continues_typing: single_byte_insert
                && cursor_followed
                && !after.text().ends_with(char::is_whitespace),
        })
    }
}

pub struct TextFieldStateInner {
    is_editing: bool,
    listeners: Vec<ChangeListener>,
    undo_stack: VecDeque<TextFieldValue>,
    redo_stack: VecDeque<TextFieldValue>,
    desired_x: Cell<Option<f32>>,
    text_revision: Cell<u64>,
    last_edit_time: Cell<Option<web_time::Instant>>,
    pending_undo_snapshot: RefCell<Option<TextFieldValue>>,
}

struct EditGuard<'a> {
    inner: &'a RefCell<TextFieldStateInner>,
}

impl<'a> EditGuard<'a> {
    fn new(inner: &'a RefCell<TextFieldStateInner>) -> Result<Self, ()> {
        {
            let borrowed = inner.borrow();
            if borrowed.is_editing {
                return Err(());
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
/// use cranpose_foundation::text::TextFieldState;
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
#[derive(Clone, Copy)]
pub struct TextFieldState {
    /// Internal state for editing machinery.
    /// Public for cross-crate pointer-based identity comparison (Hash).
    pub inner: RuntimeCell<RefCell<TextFieldStateInner>>,

    value: MutableState<TextFieldValue>,
}

impl std::fmt::Debug for TextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.read(|v| {
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
        let runtime = cranpose_core::current_runtime_handle()
            .expect("TextFieldState::new requires an active runtime");
        Self {
            inner: RuntimeCell::with_runtime(
                RefCell::new(TextFieldStateInner {
                    is_editing: false,
                    listeners: Vec::new(),
                    undo_stack: VecDeque::new(),
                    redo_stack: VecDeque::new(),
                    desired_x: Cell::new(None),
                    text_revision: Cell::new(0),
                    last_edit_time: Cell::new(None),
                    pending_undo_snapshot: RefCell::new(None),
                }),
                runtime.clone(),
            ),
            value: MutableState::with_runtime(initial_value, runtime),
        }
    }

    /// Creates a state with initial text and selection.
    pub fn with_selection(initial_text: impl Into<String>, selection: TextRange) -> Self {
        let initial_value = TextFieldValue::with_selection(initial_text, selection);
        let runtime = cranpose_core::current_runtime_handle()
            .expect("TextFieldState::with_selection requires an active runtime");
        Self {
            inner: RuntimeCell::with_runtime(
                RefCell::new(TextFieldStateInner {
                    is_editing: false,
                    listeners: Vec::new(),
                    undo_stack: VecDeque::new(),
                    redo_stack: VecDeque::new(),
                    desired_x: Cell::new(None),
                    text_revision: Cell::new(0),
                    last_edit_time: Cell::new(None),
                    pending_undo_snapshot: RefCell::new(None),
                }),
                runtime.clone(),
            ),
            value: MutableState::with_runtime(initial_value, runtime),
        }
    }

    fn inner(&self) -> Rc<RefCell<TextFieldStateInner>> {
        self.inner.get()
    }

    /// Identifies this state. Clones of the state report the same id.
    pub fn id(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    /// Where up and down arrows keep the caret: its x within its line, in
    /// layout pixels, from before the first of a run of vertical moves.
    pub fn desired_x(&self) -> Option<f32> {
        self.inner().borrow().desired_x.get()
    }

    /// Sets or clears the x that vertical moves keep; any other edit or
    /// caret move clears it.
    pub fn set_desired_x(&self, x: Option<f32>) {
        self.inner().borrow().desired_x.set(x);
    }

    /// Counts the changes to the text: it differs after each one, so a
    /// reader that keeps something built from the text knows when to build
    /// it again without comparing the text.
    pub fn text_revision(&self) -> u64 {
        self.inner().borrow().text_revision.get()
    }

    fn note_text_change(&self) {
        let inner = self.inner();
        let inner = inner.borrow();
        inner
            .text_revision
            .set(inner.text_revision.get().wrapping_add(1));
    }

    /// Reads the text in place through `f`, which must not edit this state.
    /// Creates a composition dependency when read during composition.
    pub fn with_text<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        self.value.read(|value| f(&value.text))
    }

    /// Returns the current text content.
    /// Creates composition dependency when read during composition.
    pub fn text(&self) -> String {
        self.value.read(|v| v.text.clone())
    }

    /// Returns the current selection range.
    pub fn selection(&self) -> TextRange {
        self.value.read(|v| v.selection)
    }

    /// Returns the current composition (IME) range, if any.
    pub fn composition(&self) -> Option<TextRange> {
        self.value.read(|v| v.composition)
    }

    /// Copies the selected text without modifying the clipboard.
    /// Returns the selected text, or None if no selection.
    pub fn copy_selection(&self) -> Option<String> {
        self.value.read(|v| {
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
        self.value.read(Clone::clone)
    }

    /// Adds a listener that is called when the value changes.
    ///
    /// Returns the listener index for removal.
    pub fn add_listener(&self, listener: impl Fn(&TextFieldValue) + 'static) -> usize {
        let inner_state = self.inner();
        let mut inner = inner_state.borrow_mut();
        let index = inner.listeners.len();
        inner.listeners.push(Box::new(listener));
        index
    }

    /// Sets the selection directly without going through undo stack.
    /// Use this for transient selection changes like during drag selection.
    pub fn set_selection(&self, selection: TextRange) {
        let new_value = self.value.read(|v| {
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
        !self.inner().borrow().undo_stack.is_empty()
    }

    /// Returns true if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.inner().borrow().redo_stack.is_empty()
    }

    /// Undoes the last edit.
    /// Returns true if undo was performed.
    pub fn undo(&self) -> bool {
        self.flush_undo_group();

        let inner_state = self.inner();
        let mut inner = inner_state.borrow_mut();
        if let Some(previous_state) = inner.undo_stack.pop_back() {
            let current = self.value.read(Clone::clone);
            inner.redo_stack.push_back(current);
            inner.last_edit_time.set(None);
            drop(inner);
            self.value.set(previous_state);
            self.note_text_change();
            true
        } else {
            false
        }
    }

    /// Redoes the last undone edit.
    /// Returns true if redo was performed.
    pub fn redo(&self) -> bool {
        let inner_state = self.inner();
        let mut inner = inner_state.borrow_mut();
        if let Some(redo_state) = inner.redo_stack.pop_back() {
            let current = self.value.read(Clone::clone);
            inner.undo_stack.push_back(current);
            drop(inner);
            self.value.set(redo_state);
            self.note_text_change();
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
    /// Returns `false` if another edit is already in progress.
    pub fn edit<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut TextFieldBuffer),
    {
        self.edit_as_user(None, f)
    }

    /// Edits as a person's input does: `f`, then `transformation` when `f`
    /// changed the text. Text field input edits through here, and app code
    /// through [`edit`](Self::edit).
    pub fn edit_as_user<F>(&self, transformation: Option<&InputTransformation>, f: F) -> bool
    where
        F: FnOnce(&mut TextFieldBuffer),
    {
        let inner = self.inner();
        let Ok(guard) = EditGuard::new(&inner) else {
            return false;
        };

        let mut buffer = self.value.read(|current| {
            let mut text = String::with_capacity(current.text.len() + EDIT_HEADROOM_BYTES);
            text.push_str(&current.text);
            let mut buffer = TextFieldBuffer::with_selection(text, current.selection);
            buffer.set_composition(current.composition);
            buffer
        });

        f(&mut buffer);
        if let Some(transformation) = transformation {
            self.value.read(|current| {
                if buffer.text() != current.text {
                    transformation.transform(current, &mut buffer);
                }
            });
        }

        let Some(change) = self
            .value
            .read(|current| EditChange::between(current, &buffer))
        else {
            return true;
        };

        // Undo history records text changes, as Compose's does. A move of the
        // cursor or selection only ends the typing group before it.
        if !change.text_changed {
            inner.borrow().last_edit_time.set(None);
        } else {
            let now = web_time::Instant::now();
            let mut inner = inner.borrow_mut();
            let timeout_expired = inner
                .last_edit_time
                .get()
                .is_none_or(|last| now.duration_since(last).as_millis() > UNDO_COALESCE_MS);
            // The value before this edit, while the state still holds it.
            let snapshot = || Some(self.value.read(Clone::clone));
            if timeout_expired || !change.continues_typing {
                if let Some(pending) = inner.pending_undo_snapshot.take() {
                    if inner.undo_stack.len() >= UNDO_CAPACITY {
                        inner.undo_stack.pop_front();
                    }
                    inner.undo_stack.push_back(pending);
                }
                inner.pending_undo_snapshot.replace(snapshot());
            } else if inner.pending_undo_snapshot.borrow().is_none() {
                inner.pending_undo_snapshot.replace(snapshot());
            }
            inner.redo_stack.clear();
            inner.last_edit_time.set(Some(now));
        }

        self.value.set(buffer.into_value());
        if change.text_changed {
            self.note_text_change();
        }

        drop(guard);

        let listener_count = inner.borrow().listeners.len();
        if listener_count > 0 {
            let value = self.value.get_non_reactive();
            for i in 0..listener_count {
                let inner = inner.borrow();
                if i < inner.listeners.len() {
                    (inner.listeners[i])(&value);
                }
            }
        }
        true
    }

    /// Flushes any pending undo snapshot to the undo stack.
    /// Call this when a coalescing break is desired (e.g., focus lost).
    pub fn flush_undo_group(&self) {
        let inner_state = self.inner();
        let inner = inner_state.borrow();
        if let Some(snapshot) = inner.pending_undo_snapshot.take() {
            drop(inner);
            let inner_state = self.inner();
            let mut inner = inner_state.borrow_mut();
            if inner.undo_stack.len() >= UNDO_CAPACITY {
                inner.undo_stack.pop_front();
            }
            inner.undo_stack.push_back(snapshot);
        }
    }

    /// Sets the text and places cursor at end.
    pub fn set_text(&self, text: impl Into<String>) -> bool {
        let text = text.into();
        self.edit(|buffer| {
            buffer.clear();
            buffer.insert(&text);
        })
    }
}

impl Default for TextFieldState {
    fn default() -> Self {
        Self::new("")
    }
}

impl PartialEq for TextFieldState {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

#[cfg(test)]
#[path = "tests/state_tests.rs"]
mod tests;
