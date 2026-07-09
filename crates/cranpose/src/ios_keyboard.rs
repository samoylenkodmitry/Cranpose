//! iOS soft keyboard + text input.
//!
//! winit-uikit renders to a Metal layer with no editable views, so there is no
//! first responder to raise the keyboard. This installs a hidden
//! `UIKeyInput` view: when a Cranpose text field gains focus the framework calls
//! [`PlatformTextInputHandler::show_keyboard`], we make the view first responder
//! (the keyboard rises), and its `insertText:`/`deleteBackward` callbacks queue
//! edits that the iOS event loop drains into `AppShell` each frame.
#![allow(unsafe_code)]

use cranpose_ui::text_input_session::{set_platform_text_input_handler, PlatformTextInputHandler};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSObjectProtocol, NSString};
use objc2_ui_kit::{UIKeyInput, UITextInputTraits, UIView};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// One edit forwarded from the key-input view, drained by the iOS event loop.
pub(crate) enum KeyInput {
    /// Insert literal text (a typed character, or `\n` for Return).
    Insert(String),
    /// Backspace / delete the character before the caret.
    Backspace,
}

fn queue() -> &'static Mutex<Vec<KeyInput>> {
    static Q: OnceLock<Mutex<Vec<KeyInput>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

fn push(input: KeyInput) {
    if let Ok(mut q) = queue().lock() {
        q.push(input);
    }
}

/// Drain the pending text edits (called by the iOS event loop each frame).
pub(crate) fn take_key_inputs() -> Vec<KeyInput> {
    queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

thread_local! {
    /// Keeps the input view alive for the app's lifetime.
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
            true
        }

        #[unsafe(method(insertText:))]
        fn insert_text(&self, text: &NSString) {
            push(KeyInput::Insert(text.to_string()));
        }

        #[unsafe(method(deleteBackward))]
        fn delete_backward(&self) {
            push(KeyInput::Backspace);
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
        self.view.becomeFirstResponder();
    }

    fn hide_keyboard(&self) {
        self.view.resignFirstResponder();
    }
}

/// Installs the iOS keyboard handler for the current app context. Called by the
/// iOS backend inside `AppShell::app_context().enter(..)` (the handler is stored
/// per context, like the clipboard).
pub(crate) fn register() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let view = KeyInputView::new(mtm);
    // Attach the (zero-size, invisible) view to the key window so it is in the
    // responder chain and can become first responder.
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
