//! iOS soft keyboard + full text input.
//!
//! winit-uikit renders to a Metal layer with no editable views, so there is no
//! first responder to raise the keyboard. This installs a hidden view that
//! conforms to `UITextInput` (a superset of `UIKeyInput`): focusing a Cranpose
//! text field makes it first responder (the keyboard rises), and UIKit's text
//! reads/edits are bridged to the framework's IME state.
//!
//! `UITextInput` is synchronous, but the composition's text model is only
//! touched per frame, so a synchronous [`mirror`](Mirror) of the focused field's
//! text + selection is kept: it is refreshed from `AppShell::ime_editor_state`
//! each frame (see `ios.rs`), read synchronously by the protocol methods, and
//! updated optimistically on edits (which are also queued to the shell). This is
//! what makes swipe-the-spacebar cursor movement and selection work.
#![allow(unsafe_code)]

use block2::RcBlock;
use core::ptr::NonNull;
use cranpose_ui::text_input_session::{set_platform_text_input_handler, PlatformTextInputHandler};
use cranpose_ui::EdgeInsets;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSArray, NSComparisonResult, NSNotification, NSNotificationCenter, NSObjectProtocol, NSRange,
    NSString, NSValue,
};
use objc2_ui_kit::{
    NSValueUIGeometryExtensions, NSWritingDirection, UIKeyInput, UIKeyboardFrameEndUserInfoKey,
    UIKeyboardWillChangeFrameNotification, UIResponder, UIScreen, UITextInput,
    UITextInputStringTokenizer, UITextInputTokenizer, UITextInputTraits, UITextLayoutDirection,
    UITextPosition, UITextRange, UITextSelectionRect, UITextStorageDirection, UIView,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::OnceLock;

// --------------------------------------------------------------- text mirror

/// A synchronous snapshot of the focused field's text + selection (byte offsets
/// into `text`, `sel.0 <= sel.1`). Read by `UITextInput` methods; refreshed from
/// the shell each frame and updated optimistically on edits.
#[derive(Default, Clone)]
struct Mirror {
    text: String,
    sel: (usize, usize),
}

fn mirror() -> &'static Mutex<Mirror> {
    static M: OnceLock<Mutex<Mirror>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Mirror::default()))
}

/// Refresh the mirror from the composition (called each frame in `ios.rs`).
pub(crate) fn set_mirror(text: String, sel_start: usize, sel_end: usize) {
    if let Ok(mut m) = mirror().lock() {
        let len = text.len();
        m.text = text;
        m.sel = (sel_start.min(len), sel_end.min(len));
    }
}

fn read_mirror() -> Mirror {
    mirror().lock().map(|m| m.clone()).unwrap_or_default()
}

// ---------------------------------------------- caret geometry mirror

/// Window-space caret geometry of the focused single-line field, refreshed each
/// frame from `AppShell::ime_caret_geometry`. `caret_xs[k]` is the caret x (in
/// the view's coordinate space, which matches the window) after `k` characters,
/// so `caret_xs.len() == chars + 1`. Read by the `UITextInput` geometry methods
/// so the iOS spacebar-trackpad cursor and tap-to-position can place the caret.
#[derive(Default, Clone)]
struct CaretGeom {
    caret_xs: Vec<f32>,
    top: f32,
    line_height: f32,
}

fn caret_geom() -> &'static Mutex<CaretGeom> {
    static G: OnceLock<Mutex<CaretGeom>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(CaretGeom::default()))
}

/// Refresh the caret geometry from the composition (called each frame in `ios.rs`).
pub(crate) fn set_caret_geometry(caret_xs: Vec<f32>, top: f32, line_height: f32) {
    if let Ok(mut g) = caret_geom().lock() {
        g.caret_xs = caret_xs;
        g.top = top;
        g.line_height = line_height;
    }
}

fn read_caret_geom() -> CaretGeom {
    caret_geom().lock().map(|g| g.clone()).unwrap_or_default()
}

/// Height to draw the caret at, guarding against an unmeasured (zero) line.
fn caret_height(geom: &CaretGeom) -> f64 {
    if geom.line_height > 0.5 {
        geom.line_height as f64
    } else {
        16.0
    }
}

/// Caret x (window space) for a byte offset: map the byte to a character index
/// into `caret_xs`. Falls back to the last known x when geometry is unavailable.
fn caret_x_for_byte(geom: &CaretGeom, text: &str, byte: usize) -> f32 {
    if geom.caret_xs.is_empty() {
        return 0.0;
    }
    let byte = byte.min(text.len());
    let char_index = text[..byte].chars().count();
    geom.caret_xs
        .get(char_index)
        .or_else(|| geom.caret_xs.last())
        .copied()
        .unwrap_or(0.0)
}

/// Byte offset whose caret x is closest to `x` (window space) — the inverse of
/// [`caret_x_for_byte`], used for tap-to-position and trackpad cursor movement.
fn byte_for_x(geom: &CaretGeom, text: &str, x: f32) -> usize {
    if geom.caret_xs.is_empty() {
        return text.len();
    }
    let mut best_k = 0usize;
    let mut best_d = f32::INFINITY;
    for (k, cx) in geom.caret_xs.iter().enumerate() {
        let d = (cx - x).abs();
        if d < best_d {
            best_d = d;
            best_k = k;
        }
    }
    // `caret_xs[k]` is the caret after `k` characters, i.e. the start byte of the
    // k-th character (or the text end when `k == chars`).
    text.char_indices()
        .nth(best_k)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// An edit forwarded to the composition, applied on the next frame.
pub(crate) enum ImeOp {
    /// Replace `start..end` (bytes) with `text`.
    Replace(usize, usize, String),
    /// Move the selection to `start..end` (bytes).
    SetSelection(usize, usize),
}

fn ops() -> &'static Mutex<Vec<ImeOp>> {
    static Q: OnceLock<Mutex<Vec<ImeOp>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

/// Drain queued IME edits (called by the iOS event loop each frame).
pub(crate) fn take_ime_ops() -> Vec<ImeOp> {
    ops()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

type WakeFn = Box<dyn Fn() + Send + Sync>;

fn wake_slot() -> &'static Mutex<Option<WakeFn>> {
    static W: OnceLock<Mutex<Option<WakeFn>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(None))
}

/// Installs a callback that wakes the iOS event loop so a queued edit is applied
/// on the next frame instead of waiting for the cursor-blink timer.
pub(crate) fn set_wake(wake: WakeFn) {
    if let Ok(mut w) = wake_slot().lock() {
        *w = Some(wake);
    }
}

fn queue_op(op: ImeOp) {
    if let Ok(mut q) = ops().lock() {
        q.push(op);
    }
    if let Ok(w) = wake_slot().lock() {
        if let Some(wake) = w.as_ref() {
            wake();
        }
    }
}

/// Apply an optimistic edit to the mirror and queue it for the composition.
fn apply_replace(start: usize, end: usize, text: &str) {
    if let Ok(mut m) = mirror().lock() {
        let (start, end) = (start.min(m.text.len()), end.min(m.text.len()));
        if m.text.is_char_boundary(start) && m.text.is_char_boundary(end) && start <= end {
            m.text.replace_range(start..end, text);
            let caret = start + text.len();
            m.sel = (caret, caret);
        }
    }
    queue_op(ImeOp::Replace(start, end, text.to_owned()));
}

fn set_selection(start: usize, end: usize) {
    if let Ok(mut m) = mirror().lock() {
        let len = m.text.len();
        m.sel = (start.min(len), end.min(len));
    }
    queue_op(ImeOp::SetSelection(start, end));
}

// ---------------------------------------------------- UTF-16-aware offsets

/// Byte offset that is `count` UTF-16 code units after `from` (or before, if
/// negative), clamped to the text and to a char boundary. UIKit counts text
/// positions in UTF-16 units (NSString), while the mirror stores UTF-8 bytes.
fn shift_utf16(text: &str, from: usize, count: isize) -> usize {
    if count >= 0 {
        let mut remaining = count as usize;
        let mut byte = from;
        for ch in text[from..].chars() {
            if remaining == 0 {
                break;
            }
            let u = ch.len_utf16();
            if remaining < u {
                break;
            }
            remaining -= u;
            byte += ch.len_utf8();
        }
        byte
    } else {
        let mut remaining = (-count) as usize;
        let mut byte = from;
        for ch in text[..from].chars().rev() {
            if remaining == 0 {
                break;
            }
            let u = ch.len_utf16();
            if remaining < u {
                break;
            }
            remaining -= u;
            byte -= ch.len_utf8();
        }
        byte
    }
}

/// UTF-16 code-unit distance between two byte offsets (`to - from`).
fn utf16_offset(text: &str, from: usize, to: usize) -> isize {
    if from <= to {
        text[from..to].chars().map(|c| c.len_utf16() as isize).sum()
    } else {
        -text[to..from]
            .chars()
            .map(|c| c.len_utf16() as isize)
            .sum::<isize>()
    }
}

// ------------------------------------------------- UITextPosition subclass

define_class!(
    #[unsafe(super(UITextPosition))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeTextPosition"]
    #[ivars = usize]
    struct TextPosition;

    unsafe impl NSObjectProtocol for TextPosition {}
);

impl TextPosition {
    fn make(offset: usize, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(offset);
        unsafe { msg_send![super(this), init] }
    }
    fn offset(pos: &UITextPosition) -> usize {
        pos.downcast_ref::<TextPosition>()
            .map(|p| *p.ivars())
            .unwrap_or(0)
    }
}

// -------------------------------------------------- UITextRange subclass

define_class!(
    #[unsafe(super(UITextRange))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeTextRange"]
    #[ivars = (usize, usize)]
    struct TextRange;

    unsafe impl NSObjectProtocol for TextRange {}

    impl TextRange {
        #[unsafe(method(isEmpty))]
        fn is_empty(&self) -> bool {
            let (s, e) = *self.ivars();
            s == e
        }
        #[unsafe(method_id(start))]
        fn start(&self) -> Retained<UITextPosition> {
            let (s, _) = *self.ivars();
            let pos: Retained<TextPosition> = TextPosition::make(s, self.mtm());
            unsafe { Retained::cast_unchecked(pos) }
        }
        #[unsafe(method_id(end))]
        fn end(&self) -> Retained<UITextPosition> {
            let (_, e) = *self.ivars();
            let pos: Retained<TextPosition> = TextPosition::make(e, self.mtm());
            unsafe { Retained::cast_unchecked(pos) }
        }
    }
);

impl TextRange {
    fn make(start: usize, end: usize, mtm: MainThreadMarker) -> Retained<Self> {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let this = Self::alloc(mtm).set_ivars((start, end));
        unsafe { msg_send![super(this), init] }
    }
    fn bounds(range: &UITextRange) -> (usize, usize) {
        range
            .downcast_ref::<TextRange>()
            .map(|r| *r.ivars())
            .unwrap_or((0, 0))
    }
}

fn upos(pos: Retained<TextPosition>) -> Retained<UITextPosition> {
    unsafe { Retained::cast_unchecked(pos) }
}
fn urange(range: Retained<TextRange>) -> Retained<UITextRange> {
    unsafe { Retained::cast_unchecked(range) }
}

// ----------------------------------------------------------- the input view

thread_local! {
    static VIEW: RefCell<Option<Retained<KeyInputView>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeKeyInputView"]
    #[ivars = ()]
    struct KeyInputView;

    unsafe impl NSObjectProtocol for KeyInputView {}

    unsafe impl UITextInputTraits for KeyInputView {}

    unsafe impl UIKeyInput for KeyInputView {
        #[unsafe(method(hasText))]
        fn has_text(&self) -> bool {
            !read_mirror().text.is_empty()
        }

        #[unsafe(method(insertText:))]
        fn insert_text(&self, text: &NSString) {
            let m = read_mirror();
            apply_replace(m.sel.0, m.sel.1, &text.to_string());
        }

        #[unsafe(method(deleteBackward))]
        fn delete_backward(&self) {
            let m = read_mirror();
            if m.sel.0 != m.sel.1 {
                apply_replace(m.sel.0, m.sel.1, "");
            } else if m.sel.0 > 0 {
                let prev = shift_utf16(&m.text, m.sel.0, -1);
                apply_replace(prev, m.sel.0, "");
            }
        }
    }

    unsafe impl UITextInput for KeyInputView {
        #[unsafe(method_id(textInRange:))]
        fn text_in_range(&self, range: &UITextRange) -> Option<Retained<NSString>> {
            let m = read_mirror();
            let (s, e) = TextRange::bounds(range);
            let (s, e) = (s.min(m.text.len()), e.min(m.text.len()));
            if s <= e && m.text.is_char_boundary(s) && m.text.is_char_boundary(e) {
                Some(NSString::from_str(&m.text[s..e]))
            } else {
                Some(NSString::from_str(""))
            }
        }

        #[unsafe(method(replaceRange:withText:))]
        fn replace_range_with_text(&self, range: &UITextRange, text: &NSString) {
            let (s, e) = TextRange::bounds(range);
            apply_replace(s, e, &text.to_string());
        }

        #[unsafe(method_id(selectedTextRange))]
        fn selected_text_range(&self) -> Option<Retained<UITextRange>> {
            let m = read_mirror();
            Some(urange(TextRange::make(m.sel.0, m.sel.1, self.mtm())))
        }

        #[unsafe(method(setSelectedTextRange:))]
        fn set_selected_text_range(&self, range: Option<&UITextRange>) {
            if let Some(range) = range {
                let (s, e) = TextRange::bounds(range);
                set_selection(s, e);
            }
        }

        #[unsafe(method_id(markedTextRange))]
        fn marked_text_range(&self) -> Option<Retained<UITextRange>> {
            None
        }

        #[unsafe(method_id(markedTextStyle))]
        fn marked_text_style(
            &self,
        ) -> Option<Retained<objc2_foundation::NSDictionary<NSString, objc2::runtime::AnyObject>>> {
            None
        }

        #[unsafe(method(setMarkedTextStyle:))]
        fn set_marked_text_style(
            &self,
            _style: Option<&objc2_foundation::NSDictionary<NSString, objc2::runtime::AnyObject>>,
        ) {
        }

        #[unsafe(method(setMarkedText:selectedRange:))]
        fn set_marked_text_selected_range(&self, marked: Option<&NSString>, _sel: NSRange) {
            // No composition model: commit the text directly at the caret.
            let text = marked.map(|t| t.to_string()).unwrap_or_default();
            let m = read_mirror();
            apply_replace(m.sel.0, m.sel.1, &text);
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {}

        #[unsafe(method_id(beginningOfDocument))]
        fn beginning_of_document(&self) -> Retained<UITextPosition> {
            upos(TextPosition::make(0, self.mtm()))
        }

        #[unsafe(method_id(endOfDocument))]
        fn end_of_document(&self) -> Retained<UITextPosition> {
            upos(TextPosition::make(read_mirror().text.len(), self.mtm()))
        }

        #[unsafe(method_id(textRangeFromPosition:toPosition:))]
        fn text_range_from_to(
            &self,
            from: &UITextPosition,
            to: &UITextPosition,
        ) -> Option<Retained<UITextRange>> {
            Some(urange(TextRange::make(
                TextPosition::offset(from),
                TextPosition::offset(to),
                self.mtm(),
            )))
        }

        #[unsafe(method_id(positionFromPosition:offset:))]
        fn position_from_offset(
            &self,
            position: &UITextPosition,
            offset: isize,
        ) -> Option<Retained<UITextPosition>> {
            let m = read_mirror();
            let byte = shift_utf16(&m.text, TextPosition::offset(position).min(m.text.len()), offset);
            Some(upos(TextPosition::make(byte, self.mtm())))
        }

        #[unsafe(method_id(positionFromPosition:inDirection:offset:))]
        fn position_from_in_direction_offset(
            &self,
            position: &UITextPosition,
            direction: UITextLayoutDirection,
            offset: isize,
        ) -> Option<Retained<UITextPosition>> {
            // Left/Up move back, Right/Down move forward (single line).
            let signed = match direction {
                UITextLayoutDirection::Left | UITextLayoutDirection::Up => -offset,
                _ => offset,
            };
            let m = read_mirror();
            let byte = shift_utf16(&m.text, TextPosition::offset(position).min(m.text.len()), signed);
            Some(upos(TextPosition::make(byte, self.mtm())))
        }

        #[unsafe(method(comparePosition:toPosition:))]
        fn compare_position(
            &self,
            position: &UITextPosition,
            other: &UITextPosition,
        ) -> NSComparisonResult {
            TextPosition::offset(position).cmp(&TextPosition::offset(other)).into()
        }

        #[unsafe(method(offsetFromPosition:toPosition:))]
        fn offset_from_to(&self, from: &UITextPosition, to: &UITextPosition) -> isize {
            let m = read_mirror();
            utf16_offset(
                &m.text,
                TextPosition::offset(from).min(m.text.len()),
                TextPosition::offset(to).min(m.text.len()),
            )
        }

        #[unsafe(method_id(tokenizer))]
        fn tokenizer(&self) -> Retained<ProtocolObject<dyn UITextInputTokenizer>> {
            let responder: &UIResponder = self;
            let tok = unsafe {
                UITextInputStringTokenizer::initWithTextInput(
                    UITextInputStringTokenizer::alloc(self.mtm()),
                    responder,
                )
            };
            ProtocolObject::from_retained(tok)
        }

        #[unsafe(method_id(positionWithinRange:farthestInDirection:))]
        fn position_within_range(
            &self,
            range: &UITextRange,
            direction: UITextLayoutDirection,
        ) -> Option<Retained<UITextPosition>> {
            let (s, e) = TextRange::bounds(range);
            let byte = match direction {
                UITextLayoutDirection::Left | UITextLayoutDirection::Up => s,
                _ => e,
            };
            Some(upos(TextPosition::make(byte, self.mtm())))
        }

        #[unsafe(method_id(characterRangeByExtendingPosition:inDirection:))]
        fn character_range_by_extending(
            &self,
            position: &UITextPosition,
            direction: UITextLayoutDirection,
        ) -> Option<Retained<UITextRange>> {
            let m = read_mirror();
            let p = TextPosition::offset(position).min(m.text.len());
            let (s, e) = match direction {
                UITextLayoutDirection::Left | UITextLayoutDirection::Up => {
                    (shift_utf16(&m.text, p, -1), p)
                }
                _ => (p, shift_utf16(&m.text, p, 1)),
            };
            Some(urange(TextRange::make(s, e, self.mtm())))
        }

        #[unsafe(method(baseWritingDirectionForPosition:inDirection:))]
        fn base_writing_direction(
            &self,
            _position: &UITextPosition,
            _direction: UITextStorageDirection,
        ) -> NSWritingDirection {
            NSWritingDirection::LeftToRight
        }

        #[unsafe(method(setBaseWritingDirection:forRange:))]
        fn set_base_writing_direction(&self, _dir: NSWritingDirection, _range: &UITextRange) {}

        #[unsafe(method(firstRectForRange:))]
        fn first_rect_for_range(&self, range: &UITextRange) -> CGRect {
            let (s, e) = TextRange::bounds(range);
            let geom = read_caret_geom();
            let text = read_mirror().text;
            let x1 = caret_x_for_byte(&geom, &text, s);
            let x2 = caret_x_for_byte(&geom, &text, e);
            CGRect::new(
                CGPoint::new(x1 as f64, geom.top as f64),
                CGSize::new((x2 - x1).max(0.0) as f64, caret_height(&geom)),
            )
        }

        #[unsafe(method(caretRectForPosition:))]
        fn caret_rect_for_position(&self, position: &UITextPosition) -> CGRect {
            let geom = read_caret_geom();
            let text = read_mirror().text;
            let x = caret_x_for_byte(&geom, &text, TextPosition::offset(position));
            CGRect::new(
                CGPoint::new(x as f64, geom.top as f64),
                CGSize::new(2.0, caret_height(&geom)),
            )
        }

        #[unsafe(method_id(selectionRectsForRange:))]
        fn selection_rects_for_range(
            &self,
            _range: &UITextRange,
        ) -> Retained<NSArray<UITextSelectionRect>> {
            NSArray::new()
        }

        #[unsafe(method_id(closestPositionToPoint:))]
        fn closest_position_to_point(&self, point: CGPoint) -> Option<Retained<UITextPosition>> {
            let geom = read_caret_geom();
            let text = read_mirror().text;
            let byte = byte_for_x(&geom, &text, point.x as f32);
            Some(upos(TextPosition::make(byte, self.mtm())))
        }

        #[unsafe(method_id(closestPositionToPoint:withinRange:))]
        fn closest_position_to_point_within(
            &self,
            point: CGPoint,
            range: &UITextRange,
        ) -> Option<Retained<UITextPosition>> {
            let (s, e) = TextRange::bounds(range);
            let geom = read_caret_geom();
            let text = read_mirror().text;
            let byte = byte_for_x(&geom, &text, point.x as f32).clamp(s, e);
            Some(upos(TextPosition::make(byte, self.mtm())))
        }

        #[unsafe(method_id(characterRangeAtPoint:))]
        fn character_range_at_point(&self, point: CGPoint) -> Option<Retained<UITextRange>> {
            let geom = read_caret_geom();
            let text = read_mirror().text;
            let byte = byte_for_x(&geom, &text, point.x as f32);
            Some(urange(TextRange::make(byte, byte, self.mtm())))
        }

        #[unsafe(method_id(inputDelegate))]
        fn input_delegate(
            &self,
        ) -> Option<Retained<ProtocolObject<dyn objc2_ui_kit::UITextInputDelegate>>> {
            None
        }

        #[unsafe(method(setInputDelegate:))]
        fn set_input_delegate(
            &self,
            _delegate: Option<&ProtocolObject<dyn objc2_ui_kit::UITextInputDelegate>>,
        ) {
        }
    }

    // A plain UIView cannot become first responder; opt in so the keyboard rises.
    impl KeyInputView {
        #[unsafe(method(canBecomeFirstResponder))]
        fn can_become_first_responder(&self) -> bool {
            true
        }
    }
);

impl KeyInputView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

struct IosKeyboard {
    view: Retained<KeyInputView>,
}

impl PlatformTextInputHandler for IosKeyboard {
    fn show_keyboard(&self) {
        if let Some(mtm) = MainThreadMarker::new() {
            if self.view.superview().is_none() {
                if let Some(root) = crate::ios_file_picker::root_view_controller(mtm) {
                    if let Some(root_view) = root.view() {
                        root_view.addSubview(&self.view);
                    }
                }
            }
        }
        self.view.becomeFirstResponder();
    }

    fn hide_keyboard(&self) {
        self.view.resignFirstResponder();
        // Resigning our proxy view isn't enough: the winit content view is also
        // in the responder chain and keeps the software keyboard up (our resign
        // returns true and `isFirstResponder` is false, yet the keyboard stays).
        // `-[UIView endEditing:]` on the window force-resigns *whatever* is the
        // first responder in the window, which reliably dismisses the keyboard.
        if let Some(window) = self.view.window() {
            let _: bool = unsafe { msg_send![&*window, endEditing: true] };
        }
    }
}

/// Installs the iOS keyboard handler for the current app context.
pub(crate) fn register() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let view = KeyInputView::new(mtm);
    if let Some(root) = crate::ios_file_picker::root_view_controller(mtm) {
        if let Some(root_view) = root.view() {
            root_view.addSubview(&view);
        }
    }
    VIEW.with(|cell| *cell.borrow_mut() = Some(view.clone()));
    set_platform_text_input_handler(
        Rc::new(IosKeyboard { view }) as Rc<dyn PlatformTextInputHandler>
    );
}

thread_local! {
    /// Keeps the keyboard-frame observer registration alive for the app's
    /// lifetime (removing it would stop the notifications).
    static KEYBOARD_OBSERVER: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        const { RefCell::new(None) };
}

/// Bottom inset, in points, that the soft keyboard covers in the given
/// `UIKeyboardWillChangeFrame` notification. The end frame is in screen
/// coordinates; when the keyboard is fully off-screen (hidden) its top is at
/// the screen bottom, so this returns 0.
fn keyboard_bottom_inset(note: &NSNotification, mtm: MainThreadMarker) -> Option<f32> {
    let info = note.userInfo()?;
    let value = info.objectForKey(unsafe { UIKeyboardFrameEndUserInfoKey })?;
    let value: &NSValue = value.downcast_ref()?;
    let frame: CGRect = unsafe { value.CGRectValue() };
    // Single-window phone app: the main screen is the keyboard's reference space.
    #[allow(deprecated)]
    let screen_height = UIScreen::mainScreen(mtm).bounds().size.height;
    Some((screen_height - frame.origin.y).max(0.0) as f32)
}

/// Observes soft-keyboard frame changes and publishes the covered height as the
/// bottom [`EdgeInsets`] the shell exposes through `local_ime_insets`. Without
/// this the keyboard overlaps the focused field and the bottom of scrollable
/// content stays unreachable (the framework's bring-into-view / content padding
/// keys off these insets). `wake` schedules a redraw so the new inset lays out.
pub(crate) fn observe_keyboard_insets(insets: Rc<Cell<EdgeInsets>>, wake: Rc<dyn Fn()>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let center = NSNotificationCenter::defaultCenter();
    let block = RcBlock::new(move |note: NonNull<NSNotification>| {
        let note = unsafe { note.as_ref() };
        let bottom = keyboard_bottom_inset(note, mtm).unwrap_or(0.0);
        let mut current = insets.get();
        if (current.bottom - bottom).abs() > 0.5 {
            current.bottom = bottom;
            insets.set(current);
            wake();
        }
    });
    // Keyboard notifications are posted on the main thread; a nil queue delivers
    // the block synchronously there, so the non-Send captured state is safe.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(UIKeyboardWillChangeFrameNotification),
            None,
            None,
            &block,
        )
    };
    KEYBOARD_OBSERVER.with(|cell| *cell.borrow_mut() = Some(token));
}
