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

/// Per-window store of the custom cursors the windowing system has accepted,
/// keyed by [`CustomPointerIcon::id`].
#[derive(Default)]
pub(crate) struct DesktopCursors {
    uploaded: HashMap<u64, CustomCursor>,
    rejected: HashSet<u64>,
    applied: HashMap<WindowId, PointerIcon>,
}

impl DesktopCursors {
    /// Sets `icon` as `window`'s cursor, uploading a custom image the first
    /// time it appears.
    pub(crate) fn apply(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window: &Arc<dyn Window>,
        icon: &PointerIcon,
    ) {
        let id = window.id();
        if offering_the_same_icon_needs_a_nudge(self.applied.get(&id), icon) {
            window.set_cursor(Cursor::Icon(a_cursor_that_is_not(icon)));
        }
        match icon {
            PointerIcon::System(system) => window.set_cursor(Cursor::Icon(*system)),
            PointerIcon::Custom(custom) => {
                if let Some(cursor) = self.custom_cursor(event_loop, custom) {
                    window.set_cursor(Cursor::Custom(cursor));
                }
            }
        }
        self.applied.insert(id, icon.clone());
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
    ) -> Option<CustomCursor> {
        let id = custom.id();
        if let Some(cursor) = self.uploaded.get(&id) {
            return Some(cursor.clone());
        }
        if self.rejected.contains(&id) {
            return None;
        }

        let image = custom.image();
        let source = CustomCursorSource::from_rgba(
            image.pixels().to_vec(),
            image.width() as u16,
            image.height() as u16,
            custom.hotspot_x() as u16,
            custom.hotspot_y() as u16,
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
