use std::{rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;

use super::{
    keyboard::{AndroidImeSession, AndroidSoftKeyboard, dispatch_android_ime_event},
    text_input::{AndroidImeEvent, AndroidImeEventQueue},
};

/// The Android input method for a host that runs its own event loop, as an immersive headset app
/// does on Android XR. A text field that takes focus asks the system for its keyboard, and the
/// keyboard's text and key events reach the field through the shell, as they do in an activity
/// that Cranpose runs.
///
/// The host calls [`install`](Self::install) once after it makes the shell, and
/// [`pump`](Self::pump) in each turn of its loop. The app's APK needs the Java sources of
/// `cranpose/android/java`, which the Gradle plugin adds.
pub struct AndroidInputMethod {
    session: Rc<AndroidImeSession>,
    queue: Arc<AndroidImeEventQueue>,
}

impl AndroidInputMethod {
    /// The input method of the activity of `app`.
    pub fn new(app: &android_activity::AndroidApp) -> Self {
        let queue = Arc::new(AndroidImeEventQueue::new());
        Self {
            session: AndroidImeSession::new(app.clone(), Arc::clone(&queue)),
            queue,
        }
    }

    /// Makes `shell` ask the system for the keyboard of a text field that takes focus, and hide it
    /// when focus leaves.
    pub fn install(&self, shell: &mut AppShell<WgpuRenderer>) {
        shell.set_platform_text_input(Rc::new(AndroidSoftKeyboard::new(Rc::clone(&self.session))));
    }

    /// Hands what the keyboard typed since the last turn to `shell`, and the focused field's
    /// text to the keyboard.
    pub fn pump(&self, shell: &mut AppShell<WgpuRenderer>) {
        for event in self.queue.drain() {
            // The keyboard of an immersive app is a panel of the system, which gives the app's
            // window no inset
            if !matches!(event, AndroidImeEvent::ImeInsetsChanged { .. }) {
                dispatch_android_ime_event(shell, event);
            }
        }
        if self.session.is_active() {
            self.session.sync_editor_state(shell.ime_editor_state());
        }
    }
}
