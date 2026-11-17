//! The panels a headset shows and the controllers' cursor on them.
//!
//! In a headset every panel of the app is drawn by one composition into one texture, the atlas,
//! one panel under the other. Each panel becomes a layer of the headset's compositor that shows
//! its part of the atlas. A panel keeps its part while its size stays: a panel that grows takes
//! a new part at the bottom.

use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
};

use cranpose::{MutableState, mutableStateOf};
use cranpose_core::{CompositionLocal, compositionLocalOf};

use crate::Pose;

// The space between two panels in the atlas, in dp, so the compositor's filter never reaches a
// neighbor's pixels.
const ATLAS_GAP: f32 = 8.0;

// Where a subspace starts: a meter in front of where the viewer's head was when the app
// started, at the same height.
pub(crate) const SUBSPACE_ORIGIN: Pose = Pose::at([0.0, 0.0, -1.0]);

// A panel as the backend sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelSlot {
    pub(crate) id: u64,
    // The panel's part of the atlas: left, top, width and height in dp.
    pub(crate) atlas: [f32; 4],
    // The panel's center and turn in the room.
    pub(crate) pose: Pose,
}

// Where a controller points on a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cursor {
    pub(crate) panel: u64,
    // The place on the panel in dp from its top left corner.
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) pressed: bool,
}

// The panels of the app in a headset. The headset's backend provides it around the app's
// content, and the spatial composables register their panels in it. Outside a headset there
// is none, and the composables draw flat.
#[derive(Clone)]
pub(crate) struct Space {
    inner: Rc<Inner>,
}

struct Inner {
    panels: RefCell<Vec<PanelSlot>>,
    next_id: Cell<u64>,
    bottom: Cell<f32>,
    cursor: MutableState<Option<Cursor>>,
}

impl PartialEq for Space {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Space {
    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // A space with no panels. It holds a state, so the composition's runtime makes it.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                panels: RefCell::new(Vec::new()),
                next_id: Cell::new(1),
                bottom: Cell::new(0.0),
                cursor: mutableStateOf(None),
            }),
        }
    }

    pub(crate) fn new_panel_id(&self) -> u64 {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        id
    }

    // Registers the panel `id` of `size` dp at `pose` and returns its part of the atlas.
    pub(crate) fn place(&self, id: u64, size: [f32; 2], pose: Pose) -> [f32; 4] {
        let mut panels = self.inner.panels.borrow_mut();
        if let Some(slot) = panels.iter_mut().find(|slot| slot.id == id) {
            slot.pose = pose;
            if slot.atlas[2..] == size {
                return slot.atlas;
            }
            slot.atlas = self.take_part(size);
            return slot.atlas;
        }
        let atlas = self.take_part(size);
        panels.push(PanelSlot { id, atlas, pose });
        atlas
    }

    fn take_part(&self, size: [f32; 2]) -> [f32; 4] {
        let top = self.inner.bottom.get();
        // Tops on even dp stay on whole pixels at the densities of one and a half and two
        self.inner
            .bottom
            .set(((top + size[1] + ATLAS_GAP) / 2.0).ceil() * 2.0);
        [0.0, top, size[0], size[1]]
    }

    // Forgets the panel `id`. The atlas starts again from its top once no panel is left.
    pub(crate) fn remove(&self, id: u64) {
        let mut panels = self.inner.panels.borrow_mut();
        panels.retain(|slot| slot.id != id);
        if panels.is_empty() {
            self.inner.bottom.set(0.0);
        }
    }

    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn panels(&self) -> Vec<PanelSlot> {
        self.inner.panels.borrow().clone()
    }

    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The width and height in dp that the atlas needs for every panel.
    pub(crate) fn atlas_size(&self) -> [f32; 2] {
        self.inner
            .panels
            .borrow()
            .iter()
            .fold([0.0, 0.0], |size, slot| {
                [
                    size[0].max(slot.atlas[0] + slot.atlas[2]),
                    size[1].max(slot.atlas[1] + slot.atlas[3]),
                ]
            })
    }

    // The cursor. A composable that reads it composes again when it moves.
    pub(crate) fn cursor(&self) -> Option<Cursor> {
        self.inner.cursor.get()
    }

    #[cfg_attr(
        not(all(feature = "openxr", target_os = "android")),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn set_cursor(&self, cursor: Option<Cursor>) {
        if self.inner.cursor.get() != cursor {
            self.inner.cursor.set(cursor);
        }
    }
}

thread_local! {
    static LOCAL_SPACE: OnceCell<CompositionLocal<Option<Space>>> = const { OnceCell::new() };
}

// The space of the headset the content shows in, or `None` outside a headset.
pub(crate) fn local_space() -> CompositionLocal<Option<Space>> {
    LOCAL_SPACE.with(|cell| cell.get_or_init(|| compositionLocalOf(|| None)).clone())
}

#[cfg(test)]
#[path = "tests/space_tests.rs"]
mod tests;
