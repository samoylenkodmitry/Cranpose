//! Applying a [`PointerIcon`] to a winit window.
//!
//! Standard shapes go straight through as a winit `CursorIcon`; a custom shape
//! has to be uploaded to the windowing system first, which is what the cache
//! here avoids repeating. A skin whose every control carries its own cursor
//! would otherwise rebuild the same handful of images on each pointer move.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use cranpose_ui::{CursorIcon, CustomPointerIcon, PointerIcon};
use winit::{
    cursor::{Cursor, CustomCursor, CustomCursorSource},
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

use crate::CustomCursorSize;

/// Whether re-offering `icon` would be dropped before it reaches the platform.
///
/// A backend that already holds the cursor being set treats the call as a
/// no-op, and on macOS that skips the cursor-rectangle invalidation with it.
/// That is exactly the case a refresh is made of: the region under the pointer
/// has not changed, so the icon offered is the one the window already has, and
/// the refresh would do nothing. Offering a different cursor first makes the
/// one that matters a change again.
fn offering_the_same_icon_needs_a_nudge(applied: Option<&PointerIcon>, icon: &PointerIcon) -> bool {
    applied.is_some_and(|applied| applied == icon)
}

/// A stock cursor that is never equal to `icon`, for the nudge.
fn a_cursor_that_is_not(icon: &PointerIcon) -> CursorIcon {
    match icon {
        PointerIcon::System(CursorIcon::Default) => CursorIcon::Pointer,
        _ => CursorIcon::Default,
    }
}

/// A custom cursor as the platform takes it: the icon's
/// [`CustomPointerIcon::id`] and the scale its image was handed over at.
type CursorKey = (u64, u64);

fn cursor_key(custom: &CustomPointerIcon, factor: f64) -> CursorKey {
    (custom.id(), factor.to_bits())
}

/// What the platform under `window` does to a custom cursor image by itself.
fn cursor_surface(window: &dyn Window) -> crate::cursor_scale::CursorSurface {
    #[cfg(target_os = "macos")]
    let (pointer_scale, image_in_points) = (crate::macos_cursor::pointer_scale(), true);
    #[cfg(target_os = "windows")]
    let (pointer_scale, image_in_points) = (crate::windows_cursor::pointer_scale(), false);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (pointer_scale, image_in_points) = (
        crate::cursor_scale::xcursor_scale(std::env::var("XCURSOR_SIZE").ok().as_deref()),
        false,
    );
    crate::cursor_scale::CursorSurface {
        pointer_scale,
        density: window.scale_factor(),
        image_in_points,
        // Only macOS enlarges an app's own cursor images; the others show
        // them pixel for pixel.
        enlarges_custom_images: image_in_points,
    }
}

/// Per-window store of the custom cursors the windowing system has accepted,
/// keyed by [`CursorKey`].
#[derive(Default)]
pub(crate) struct DesktopCursors {
    size: CustomCursorSize,
    uploaded: HashMap<CursorKey, CustomCursor>,
    rejected: HashSet<CursorKey>,
    applied: HashMap<WindowId, PointerIcon>,
    /// Cursors winit cannot build, sized in points rather than one point to a
    /// pixel.
    #[cfg(target_os = "macos")]
    as_drawn: HashMap<CursorKey, crate::macos_cursor::AsDrawnCursor>,
    /// The window under the pointer showing one of those, with its cursor
    /// rectangles held off, and which one it shows.
    #[cfg(target_os = "macos")]
    held: HashMap<WindowId, CursorKey>,
    /// The pointer moved over a held window since the cursor was last shown.
    #[cfg(target_os = "macos")]
    moved_over_held: bool,
}

impl DesktopCursors {
    pub(crate) fn new(size: CustomCursorSize) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// Sets `icon` as `window`'s cursor, uploading a custom image the first
    /// time it appears, scaled so it shows at the size [`CustomCursorSize`]
    /// asks for.
    pub(crate) fn apply(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window: &Arc<dyn Window>,
        icon: &PointerIcon,
    ) {
        let id = window.id();
        let factor = match icon {
            PointerIcon::Custom(_) => {
                crate::cursor_scale::image_scale(self.size, cursor_surface(window.as_ref()))
            }
            PointerIcon::System(_) => 1.0,
        };
        #[cfg(target_os = "macos")]
        {
            if self.hold_as_drawn(window, icon, factor) {
                self.applied.insert(id, icon.clone());
                return;
            }
            if self.held.remove(&id).is_some() {
                crate::macos_cursor::release(window.as_ref());
                // winit still holds the cursor it last set, which may be the
                // one wanted now; offering another first makes it a change.
                window.set_cursor(Cursor::Icon(a_cursor_that_is_not(icon)));
            }
        }
        if offering_the_same_icon_needs_a_nudge(self.applied.get(&id), icon) {
            window.set_cursor(Cursor::Icon(a_cursor_that_is_not(icon)));
        }
        match icon {
            PointerIcon::System(system) => window.set_cursor(Cursor::Icon(*system)),
            PointerIcon::Custom(custom) => {
                if let Some(cursor) = self.custom_cursor(event_loop, custom, factor) {
                    window.set_cursor(Cursor::Custom(cursor));
                }
            }
        }
        self.applied.insert(id, icon.clone());
    }

    /// Shows `icon` sized in points by `factor`, when it is a custom cursor
    /// winit's one point to a pixel would show at the wrong size.
    #[cfg(target_os = "macos")]
    fn hold_as_drawn(&mut self, window: &Arc<dyn Window>, icon: &PointerIcon, factor: f64) -> bool {
        let PointerIcon::Custom(custom) = icon else {
            return false;
        };
        if !crate::cursor_scale::rescales(factor) {
            return false;
        }
        let key = cursor_key(custom, factor);
        let cursor = match self.as_drawn.entry(key) {
            std::collections::hash_map::Entry::Occupied(built) => built.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                let image = custom.image();
                let Some(cursor) = crate::macos_cursor::as_drawn(
                    image.pixels(),
                    image.width(),
                    image.height(),
                    (custom.hotspot_x(), custom.hotspot_y()),
                    factor,
                ) else {
                    return false;
                };
                slot.insert(cursor)
            }
        };
        crate::macos_cursor::hold(window.as_ref(), cursor);
        self.held.insert(window.id(), key);
        true
    }

    /// The pointer moved over, or into, the window `id`.
    pub(crate) fn pointer_moved(&mut self, id: WindowId) {
        #[cfg(target_os = "macos")]
        if self.held.contains_key(&id) {
            self.moved_over_held = true;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = id;
    }

    /// Shows the held cursor again after the pointer moved over its window,
    /// when the window server may have put its own arrow over it. Called once
    /// per pass of the event loop.
    pub(crate) fn keep_held(&mut self) {
        #[cfg(target_os = "macos")]
        if std::mem::take(&mut self.moved_over_held) {
            for key in self.held.values() {
                if let Some(cursor) = self.as_drawn.get(key) {
                    crate::macos_cursor::keep(cursor);
                }
            }
        }
    }

    /// The pointer left `window`: a cursor it held goes back to the window's
    /// own rectangles, so it is not kept over whatever the pointer is over now.
    pub(crate) fn pointer_left(&mut self, window: &Arc<dyn Window>) {
        #[cfg(target_os = "macos")]
        if self.held.remove(&window.id()).is_some() {
            crate::macos_cursor::release(window.as_ref());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window;
    }

    /// The uploaded cursor for `custom`, uploading it if this is the first time
    /// it is asked for.
    ///
    /// A cursor the windowing system refuses is reported once and remembered as
    /// rejected, so the window keeps the cursor it already has instead of
    /// re-attempting the upload on every pointer move.
    fn custom_cursor(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        custom: &CustomPointerIcon,
        factor: f64,
    ) -> Option<CustomCursor> {
        let id = cursor_key(custom, factor);
        if let Some(cursor) = self.uploaded.get(&id) {
            return Some(cursor.clone());
        }
        if self.rejected.contains(&id) {
            return None;
        }

        let image = custom.image();
        let hotspot = (custom.hotspot_x(), custom.hotspot_y());
        let (pixels, width, height, hotspot) = if crate::cursor_scale::rescales(factor) {
            crate::cursor_scale::scaled_image(
                image.pixels(),
                image.width(),
                image.height(),
                hotspot,
                factor,
            )
        } else {
            (
                image.pixels().to_vec(),
                image.width(),
                image.height(),
                hotspot,
            )
        };
        let source = CustomCursorSource::from_rgba(
            pixels,
            width as u16,
            height as u16,
            hotspot.0 as u16,
            hotspot.1 as u16,
        );
        let cursor = match source {
            Ok(source) => event_loop.create_custom_cursor(source).map_err(|error| {
                log::warn!("the windowing system refused a custom cursor: {error}");
            }),
            Err(error) => {
                log::warn!("a custom cursor image was rejected: {error}");
                Err(())
            }
        };
        match cursor {
            Ok(cursor) => {
                self.uploaded.insert(id, cursor.clone());
                Some(cursor)
            }
            Err(()) => {
                self.rejected.insert(id);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cranpose_ui::{CursorIcon, PointerIcon};

    use super::{a_cursor_that_is_not, offering_the_same_icon_needs_a_nudge};

    #[test]
    fn re_offering_the_icon_a_window_already_has_needs_a_nudge() {
        let icon = PointerIcon::System(CursorIcon::Grab);

        assert!(
            offering_the_same_icon_needs_a_nudge(Some(&icon), &icon),
            "a refresh offers the icon the window already holds, and the \
             backend drops that call along with the invalidation the platform \
             needs, so the cursor would never be re-drawn"
        );
    }

    #[test]
    fn a_genuine_change_goes_straight_through() {
        let held = PointerIcon::System(CursorIcon::Grab);
        let wanted = PointerIcon::System(CursorIcon::Text);

        assert!(!offering_the_same_icon_needs_a_nudge(Some(&held), &wanted));
        assert!(!offering_the_same_icon_needs_a_nudge(None, &wanted));
    }

    #[test]
    fn the_nudge_is_never_the_icon_it_is_making_room_for() {
        for icon in [
            PointerIcon::DEFAULT,
            PointerIcon::System(CursorIcon::Grab),
            PointerIcon::System(CursorIcon::Text),
        ] {
            assert_ne!(
                PointerIcon::System(a_cursor_that_is_not(&icon)),
                icon,
                "a nudge equal to the target would be dropped exactly like the \
                 call it is making room for: {icon:?}"
            );
        }
    }
}
