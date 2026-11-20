//! The pointers' gestures on the app's composition, as the composition's pointers: each hand's
//! pointer has an id of its own, so the two reach the nodes as two pointers.

use std::fmt::Debug;

use cranpose_app_shell::AppShell;
use cranpose_render_common::Renderer;

use crate::surface_input::{Gesture, HandGesture};

// Sends `events`, in order, to the composition of `shell`. A press, a move, a release or a scroll
// of one hand does not touch the pointer of the other hand.
pub(crate) fn deliver<R>(shell: &mut AppShell<R>, events: &[HandGesture])
where
    R: Renderer,
    R::Error: Debug,
{
    for &HandGesture { hand, gesture } in events {
        let pointer = hand.pointer_id();
        match gesture {
            Gesture::Move { x, y } => shell.independent_pointer_moved(pointer, x, y),
            Gesture::Press(button) => shell.independent_pointer_pressed(pointer, button),
            Gesture::Release(button) => shell.independent_pointer_released(pointer, button),
            Gesture::Scroll { x, y } => shell.independent_pointer_scrolled(pointer, x, y),
            Gesture::Leave => shell.independent_pointer_exited(pointer),
        };
    }
}
