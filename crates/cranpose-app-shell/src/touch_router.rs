//! Fingers of a platform that tells them apart, routed to the shell's
//! pointers: the first finger down drives the primary pointer, and each
//! finger that joins it a secondary pointer of its own, until the gesture
//! ends.

use std::fmt::Debug;

use cranpose_foundation::PointerId;
use cranpose_render_common::Renderer;

use crate::{AppShell, PointerSource};

/// Which shell pointer a finger drives.
enum Route {
    Primary,
    Secondary(PointerId),
    Untracked,
}

/// Routes the fingers on one surface to the shell's primary and secondary
/// pointers. `F` is the platform's identity of a finger. `None` stands for
/// a mouse, which always drives the primary pointer.
///
/// Each method returns whether the shell changed and needs a frame.
#[derive(Debug)]
pub struct TouchRouter<F> {
    primary: Option<F>,
    /// The fingers that joined the primary one, in the order they came,
    /// with the secondary pointer each drives.
    secondaries: Vec<(F, PointerId)>,
    next_secondary_id: PointerId,
}

impl<F> Default for TouchRouter<F> {
    fn default() -> Self {
        Self {
            primary: None,
            secondaries: Vec::new(),
            next_secondary_id: PointerId::PRIMARY,
        }
    }
}

impl<F: Copy + Eq> TouchRouter<F> {
    /// A finger, or the mouse, went down at `(x, y)` in logical pixels.
    pub fn press<R>(&mut self, shell: &mut AppShell<R>, finger: Option<F>, x: f32, y: f32) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        match self.route_press(finger) {
            Route::Primary => {
                let event_time = shell.realtime_pointer_event_time(None);
                shell.set_cursor_at_event_time(x, y, event_time);
                shell.pointer_pressed_at_event_time(event_time)
            }
            Route::Secondary(id) => shell.secondary_pointer_pressed(id, x, y, None),
            Route::Untracked => false,
        }
    }

    /// A finger, or the mouse, moved to `(x, y)`. A finger that is not down
    /// moves nothing.
    pub fn moved<R>(&self, shell: &mut AppShell<R>, finger: Option<F>, x: f32, y: f32) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        match self.route(finger) {
            Route::Primary => shell.set_cursor(x, y),
            Route::Secondary(id) => shell.secondary_pointer_moved(id, x, y, None),
            Route::Untracked => false,
        }
    }

    /// A finger, or the mouse, lifted at `(x, y)`. The primary finger's
    /// lift ends the gesture: the secondary fingers are released first,
    /// there, and the next finger down drives the primary pointer.
    pub fn release<R>(&mut self, shell: &mut AppShell<R>, finger: Option<F>, x: f32, y: f32) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let Some(finger) = finger else {
            return release_primary(shell, x, y);
        };
        if self.primary == Some(finger) {
            let mut changed = false;
            for (_, id) in self.secondaries.drain(..) {
                changed |= shell.secondary_pointer_released(id, x, y, None);
            }
            self.end_gesture();
            return release_primary(shell, x, y) | changed;
        }
        if let Some(id) = self.take_secondary(finger) {
            return shell.secondary_pointer_released(id, x, y, None);
        }
        // A lift after the gesture ended, as some platforms send one per
        // finger: only a lift with no gesture running reaches the shell.
        self.is_idle() && release_primary(shell, x, y)
    }

    /// A finger, or the mouse, left without lifting: the system took the
    /// touch, or it left the surface. The primary finger's leaving cancels
    /// the gesture. A secondary finger is released at `position`, or at the
    /// origin when the platform gives none.
    pub fn cancel<R>(
        &mut self,
        shell: &mut AppShell<R>,
        finger: Option<F>,
        position: Option<(f32, f32)>,
    ) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let secondary = finger
            .filter(|finger| self.primary != Some(*finger))
            .and_then(|finger| self.take_secondary(finger));
        if let Some(id) = secondary {
            shell.set_pointer_source(PointerSource::Touch);
            let (x, y) = position.unwrap_or((0.0, 0.0));
            return shell.secondary_pointer_released(id, x, y, None);
        }
        let ends_gesture = finger.is_none() || self.primary == finger || self.is_idle();
        if ends_gesture {
            self.end_gesture();
            shell.cancel_gesture();
        }
        ends_gesture
    }

    fn route_press(&mut self, finger: Option<F>) -> Route {
        let Some(finger) = finger else {
            return Route::Primary;
        };
        match self.primary {
            None => {
                self.primary = Some(finger);
                Route::Primary
            }
            Some(primary) if primary == finger => Route::Primary,
            Some(_) => Route::Secondary(self.secondary_id(finger).unwrap_or_else(|| {
                self.next_secondary_id = PointerId(self.next_secondary_id.0 + 1);
                self.secondaries.push((finger, self.next_secondary_id));
                self.next_secondary_id
            })),
        }
    }

    fn route(&self, finger: Option<F>) -> Route {
        match finger {
            None => Route::Primary,
            Some(finger) if self.primary == Some(finger) => Route::Primary,
            Some(finger) => self
                .secondary_id(finger)
                .map_or(Route::Untracked, Route::Secondary),
        }
    }

    fn secondary_id(&self, finger: F) -> Option<PointerId> {
        self.secondaries
            .iter()
            .find(|(tracked, _)| *tracked == finger)
            .map(|(_, id)| *id)
    }

    fn take_secondary(&mut self, finger: F) -> Option<PointerId> {
        let index = self
            .secondaries
            .iter()
            .position(|(tracked, _)| *tracked == finger)?;
        Some(self.secondaries.remove(index).1)
    }

    fn is_idle(&self) -> bool {
        self.primary.is_none() && self.secondaries.is_empty()
    }

    fn end_gesture(&mut self) {
        self.primary = None;
        self.secondaries.clear();
        self.next_secondary_id = PointerId::PRIMARY;
    }
}

fn release_primary<R>(shell: &mut AppShell<R>, x: f32, y: f32) -> bool
where
    R: Renderer,
    R::Error: Debug,
{
    let event_time = shell.realtime_pointer_event_time(None);
    shell.pointer_released_at_position_event_time(x, y, event_time)
}
