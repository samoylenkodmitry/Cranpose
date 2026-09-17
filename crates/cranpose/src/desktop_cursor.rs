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

use cranpose_ui::{CustomPointerIcon, PointerIcon};
use winit::{
    cursor::{Cursor, CustomCursor, CustomCursorSource},
    event_loop::ActiveEventLoop,
    window::Window,
};

/// Per-window store of the custom cursors the windowing system has accepted,
/// keyed by [`CustomPointerIcon::id`].
#[derive(Default)]
pub(crate) struct DesktopCursors {
    uploaded: HashMap<u64, CustomCursor>,
    rejected: HashSet<u64>,
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
        match icon {
            PointerIcon::System(system) => window.set_cursor(Cursor::Icon(*system)),
            PointerIcon::Custom(custom) => {
                if let Some(cursor) = self.custom_cursor(event_loop, custom) {
                    window.set_cursor(Cursor::Custom(cursor));
                }
            }
        }
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
