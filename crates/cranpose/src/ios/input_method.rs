//! The system keyboard on iOS: a keyboard extension whose principal class is
//! `CranposeInputViewController`. Its view holds a `CAMetalLayer`, and the
//! board draws into it on the GPU on the frames of a `CADisplayLink` while the
//! board asks for frames.
//!
//! Everything runs on the extension's main thread. Only the frame waker may
//! run elsewhere; it wakes the display link through the main queue.
#![expect(
    unsafe_code,
    reason = "Objective-C messages to UIKit and NSExtensionMain"
)]

use std::ffi::{CString, c_char, c_int, c_void};

use cranpose_ui::Size;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::{
    ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{AnyClass, ProtocolObject},
    sel,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes, NSSet, NSString, NSUUID,
};
use objc2_quartz_core::{CADisplayLink, CAMetalLayer};
use objc2_ui_kit::{
    NSLayoutConstraint, UIEvent, UIInputViewController, UIKeyInput, UIResponder, UIScreen,
    UITextDocumentProxy, UITextInput, UITextInputDelegate, UITextInputTraits, UITouch, UIView,
    UIViewAutoresizing, UIViewController,
};

use crate::{
    AppSettings, GpuContext, LaunchError,
    keyboard_host::{FrameRequest, KeyboardState, KeyboardWindow, TouchPhase},
    uikit_editor_info::{TextTraits, editor_info},
};

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    fn NSExtensionMain(argc: c_int, argv: *const *const c_char) -> c_int;
}

// The layer of the board's view.
pub(crate) struct KeyboardLayer(Retained<CAMetalLayer>);

#[derive(Debug, thiserror::Error)]
pub(crate) enum KeyboardSurfaceError {
    #[error("the keyboard's surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("no GPU device for the keyboard: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("the keyboard's surface configuration: {0}")]
    Config(#[from] LaunchError),
}

impl KeyboardWindow for KeyboardLayer {
    type Error = KeyboardSurfaceError;

    fn same(&self, other: &Self) -> bool {
        Retained::as_ptr(&self.0) == Retained::as_ptr(&other.0)
    }

    fn instance(_settings: &AppSettings) -> wgpu::Instance {
        crate::wgpu_surface::new_instance()
    }

    fn device(
        adapter: &wgpu::Adapter,
    ) -> Result<(wgpu::Device, wgpu::Queue), KeyboardSurfaceError> {
        Ok(pollster::block_on(adapter.request_device(
            &super::event_loop::ios_device_descriptor(adapter),
        ))?)
    }

    fn surface(
        &self,
        gpu: &GpuContext,
        (width, height): (u32, u32),
    ) -> Result<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration), KeyboardSurfaceError> {
        let layer = Retained::as_ptr(&self.0).cast_mut().cast::<c_void>();
        // SAFETY: the layer outlives the surface: the host keeps both in one
        // target and drops the surface first.
        let surface = unsafe {
            gpu.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }?;
        // Each image comes back with the frame it presented, so a frame draws
        // only what changed since.
        surface.keep_presented_images(true);
        let config = super::event_loop::ios_surface_config(&surface, &gpu.adapter, width, height)?;
        surface.configure(&gpu.device, &config);
        Ok((surface, config))
    }

    fn request_frame() {
        DispatchQueue::main().exec_async(wake);
    }
}

// The controller on screen and what it shows the board with.
pub(crate) struct KeyboardSession {
    controller: Weak<KeyboardController>,
    view: Retained<KeyboardView>,
    link: Option<Retained<CADisplayLink>>,
    height: Option<Retained<NSLayoutConstraint>>,
    // The document of the focused field, which changes with the field.
    document: Option<Retained<NSUUID>>,
}

fn with_keyboard<R>(f: impl FnOnce(&KeyboardState<KeyboardLayer>) -> R) -> R {
    crate::thread_state::with(|thread| f(&thread.input_method))
}

fn with_session<R>(f: impl FnOnce(&mut KeyboardSession) -> R) -> Option<R> {
    crate::thread_state::with(|thread| thread.ios.keyboard_session.borrow_mut().as_mut().map(f))
}

// The session of `controller`, and none for a controller UIKit replaced.
fn with_own_session<R>(
    controller: &KeyboardController,
    f: impl FnOnce(&mut KeyboardSession) -> R,
) -> Option<R> {
    with_session(|session| {
        let own = session
            .controller
            .load()
            .is_some_and(|current| std::ptr::eq(&*current, controller));
        own.then(|| f(session))
    })
    .flatten()
}

fn current_controller() -> Option<Retained<KeyboardController>> {
    with_session(|session| session.controller.load()).flatten()
}

// Runs the board in `settings` as the system keyboard, from the extension's
// main function.
pub(crate) fn run(settings: AppSettings) -> ! {
    crate::apple::mobile::init_logging();
    super::host::register();
    super::app_info::register();
    super::device_info::register();
    crate::process_info::install();
    settings.start_process();
    with_keyboard(|keyboard| keyboard.install(settings));
    // NSExtensionMain finds the principal class by name.
    let _ = KeyboardController::class();
    let arguments: Vec<CString> = std::env::args()
        .filter_map(|argument| CString::new(argument).ok())
        .collect();
    let pointers: Vec<*const c_char> = arguments.iter().map(|argument| argument.as_ptr()).collect();
    let count = c_int::try_from(pointers.len()).unwrap_or(c_int::MAX);
    // SAFETY: the arguments live until the process ends, which
    // NSExtensionMain does not return before.
    let code = unsafe { NSExtensionMain(count, pointers.as_ptr()) };
    std::process::exit(code)
}

define_class!(
    #[unsafe(super(UIView, UIResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeKeyboardView"]
    #[ivars = ()]
    pub(crate) struct KeyboardView;

    unsafe impl NSObjectProtocol for KeyboardView {}

    impl KeyboardView {
        #[unsafe(method(layerClass))]
        fn layer_class() -> &'static AnyClass {
            CAMetalLayer::class()
        }

        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };
            self.surface_changed();
        }

        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.touch(touches, TouchPhase::Down);
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.touch(touches, TouchPhase::Move);
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.touch(touches, TouchPhase::Up);
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            self.touch(touches, TouchPhase::Cancel);
        }
    }
);

impl KeyboardView {
    fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view.setAutoresizingMask(
            UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
        );
        view.setMultipleTouchEnabled(true);
        // A view backed by a CAMetalLayer starts at one pixel per point.
        view.setContentScaleFactor(main_screen(mtm).scale());
        view
    }

    fn surface_changed(&self) {
        let scale = self.contentScaleFactor();
        let bounds = self.bounds();
        let pixels = |points: f64| (points * scale).round().max(0.0) as u32;
        // UIKit lays the view out at the whole screen before the board's
        // height constraint takes hold; a frame drawn then lays out and
        // draws the board at the screen's size, only to draw it again.
        if bounds.size.height >= main_screen(self.mtm()).bounds().size.height {
            return;
        }
        let size = (pixels(bounds.size.width), pixels(bounds.size.height));
        let Ok(layer) = self.layer().downcast::<CAMetalLayer>() else {
            log::error!("the keyboard's view has no CAMetalLayer");
            return;
        };
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        let changed = with_keyboard(|keyboard| {
            keyboard.surface_changed(KeyboardLayer(layer), size, scale as f32)
        });
        match changed {
            Ok(()) => wake(),
            Err(error) => log::error!("the keyboard cannot draw: {error}"),
        }
    }

    fn touch(&self, touches: &NSSet<UITouch>, phase: TouchPhase) {
        let scale = self.contentScaleFactor();
        for touch in touches {
            let point = touch.locationInView(Some(self));
            // A touch is the same object from its start to its end.
            let pointer = Retained::as_ptr(&touch) as u64;
            with_keyboard(|keyboard| {
                keyboard.touch(
                    pointer,
                    phase,
                    (point.x * scale) as f32,
                    (point.y * scale) as f32,
                );
            });
        }
        wake();
    }
}

define_class!(
    #[unsafe(super(UIInputViewController, UIViewController, UIResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeInputViewController"]
    #[ivars = ()]
    pub(crate) struct KeyboardController;

    unsafe impl NSObjectProtocol for KeyboardController {}

    unsafe impl UITextInputDelegate for KeyboardController {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, _text_input: Option<&ProtocolObject<dyn UITextInput>>) {
            self.read_field();
        }
    }

    impl KeyboardController {
        #[unsafe(method(viewDidLoad))]
        fn view_did_load(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidLoad] };
            let Some(root) = self.view() else {
                return;
            };
            let view = KeyboardView::new(self.mtm(), root.bounds());
            root.addSubview(&view);
            crate::thread_state::with(|thread| {
                let mut slot = thread.ios.keyboard_session.borrow_mut();
                // UIKit may load a new controller before the old one leaves;
                // the old link would keep firing and keep the old controller.
                if let Some(link) = slot.as_mut().and_then(|old| old.link.take()) {
                    link.invalidate();
                }
                *slot = Some(KeyboardSession {
                    controller: Weak::new(self),
                    view,
                    link: None,
                    height: None,
                    document: None,
                });
            });
            self.hold_height();
        }

        #[unsafe(method(viewWillAppear:))]
        fn view_will_appear(&self, animated: bool) {
            let _: () = unsafe { msg_send![super(self), viewWillAppear: animated] };
            with_own_session(self, |session| {
                session.document = None;
                // The board left its surface when it hid; the layout makes one again.
                session.view.setNeedsLayout();
            });
            self.read_field();
            self.start_frames();
            with_keyboard(|keyboard| keyboard.set_visible(true));
        }

        #[unsafe(method(viewDidDisappear:))]
        fn view_did_disappear(&self, animated: bool) {
            let _: () = unsafe { msg_send![super(self), viewDidDisappear: animated] };
            let own = with_own_session(self, |session| {
                // The link holds its target, so it goes before the controller can.
                if let Some(link) = session.link.take() {
                    link.invalidate();
                }
            });
            if own.is_some() {
                // A hidden board keeps no surface images.
                with_keyboard(KeyboardState::surface_destroyed);
            }
        }

        #[unsafe(method(step:))]
        fn step(&self, _link: &CADisplayLink) {
            let request = with_keyboard(KeyboardState::frame);
            let paused = request != FrameRequest::Next;
            with_own_session(self, |session| {
                if let Some(link) = &session.link {
                    link.setPaused(paused);
                }
            });
            if let FrameRequest::After(wait) = request
                && let Ok(when) = DispatchTime::try_from(wait)
            {
                let _ = DispatchQueue::main().after(when, wake);
            }
        }
    }
);

impl KeyboardController {
    // Keeps the board at the height the application names for this screen.
    fn hold_height(&self) {
        let Some(root) = self.view() else {
            return;
        };
        let screen = main_screen(self.mtm()).bounds().size;
        let Some(height) = with_keyboard(|keyboard| {
            keyboard.height(Size {
                width: screen.width as f32,
                height: screen.height as f32,
            })
        }) else {
            return;
        };
        with_own_session(self, |session| {
            if let Some(old) = session.height.take() {
                old.setActive(false);
            }
            let constraint = root
                .heightAnchor()
                .constraintEqualToConstant(f64::from(height));
            // UILayoutPriorityDefaultHigh.
            constraint.setPriority(750.0);
            constraint.setActive(true);
            session.height = Some(constraint);
        });
    }

    fn start_frames(&self) {
        let link = unsafe { CADisplayLink::displayLinkWithTarget_selector(self, sel!(step:)) };
        // SAFETY: the main run loop runs as long as the extension.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        with_own_session(self, |session| {
            if let Some(old) = session.link.replace(link) {
                old.invalidate();
            }
        });
    }

    // Reads the focused field from the text proxy. A new document means a
    // new field took the keyboard.
    fn read_field(&self) {
        let proxy = self.textDocumentProxy();
        let traits = TextTraits {
            keyboard_type: proxy.keyboardType().0,
            return_key_type: proxy.returnKeyType().0,
            autocapitalization: proxy.autocapitalizationType().0,
            secure: proxy.isSecureTextEntry(),
        };
        let before = proxy
            .documentContextBeforeInput()
            .map_or_else(String::new, |text| text.to_string());
        let editor = editor_info(traits, &before);
        let document = proxy.documentIdentifier();
        let started = with_own_session(self, |session| {
            let same = session
                .document
                .as_ref()
                .is_some_and(|current| current.isEqual(Some(&document)));
            session.document = Some(document);
            !same
        });
        with_keyboard(|keyboard| match started {
            Some(true) => keyboard.start_input(editor),
            _ => keyboard.update_editor(editor),
        });
        wake();
    }
}

#[expect(
    deprecated,
    reason = "a keyboard extension's view has no window scene before it appears"
)]
fn main_screen(mtm: MainThreadMarker) -> Retained<UIScreen> {
    UIScreen::mainScreen(mtm)
}

// Lets the display link draw the next frame.
fn wake() {
    with_session(|session| {
        if let Some(link) = &session.link {
            link.setPaused(false);
        }
    });
}

fn with_proxy(f: impl FnOnce(&ProtocolObject<dyn UITextDocumentProxy>)) {
    match current_controller() {
        Some(controller) => f(&controller.textDocumentProxy()),
        None => log::warn!("the keyboard has no field to type into"),
    }
}

pub(crate) fn commit_text(text: &str) {
    with_proxy(|proxy| proxy.insertText(&NSString::from_str(text)));
}

pub(crate) fn delete_backward() {
    with_proxy(UIKeyInput::deleteBackward);
}

// A field runs its return key's action when it receives a new line.
pub(crate) fn perform_ime_action() {
    commit_text("\n");
}

pub(crate) fn switch_to_next_keyboard() {
    if let Some(controller) = current_controller() {
        controller.advanceToNextInputMode();
    }
}
