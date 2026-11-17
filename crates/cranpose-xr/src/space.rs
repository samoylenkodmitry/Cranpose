//! The panels a headset shows and the controllers' cursor on them.
//!
//! In a headset every panel of the app is drawn by one composition into one texture, the atlas,
//! one panel under the other in the order the panels registered. Each panel becomes a layer of
//! the headset's compositor that shows its part of the atlas. A panel keeps its part while its
//! size and the sizes of the panels above it stay. When a panel changes its size or leaves, the
//! panels stack again from the top, so the atlas needs no more room than its panels.

use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
};

use cranpose::{ExternalSurface, MutableState, mutableStateOf};
use cranpose_core::{CompositionLocal, compositionLocalOf};

use crate::{Pose, StereoMode};

// The space between two panels in the atlas, in dp, so the compositor's filter never reaches a
// neighbor's pixels.
const ATLAS_GAP: f32 = 8.0;

// The step in dp by which the dot of a cursor moves. A smaller move leaves the dot where it is.
const CURSOR_STEP: f32 = 0.5;

// Where a subspace starts: a meter in front of where the viewer's head was when the app
// started, at the same height.
pub(crate) const SUBSPACE_ORIGIN: Pose = Pose::at([0.0, 0.0, -1.0]);

// What a panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelKind {
    // Cranpose content, drawn into the atlas.
    Content,
    // A surface that another producer writes into. It has no part of the atlas, and no
    // composition: a ray stops on it and presses nothing.
    Surface(StereoMode),
}

// A panel as the backend sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelSlot {
    pub(crate) id: u64,
    pub(crate) kind: PanelKind,
    // The panel's part of the atlas: left, top, width and height in dp. A surface holds its size
    // here and no room in the atlas.
    pub(crate) atlas: [f32; 4],
    // The panel's center and turn in the room.
    pub(crate) pose: Pose,
    // Whether the user may carry the panel with a controller.
    pub(crate) mobility: Mobility,
}

impl PanelSlot {
    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The panel's width and height in dp.
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.atlas[2], self.atlas[3]]
    }
}

// Whether the user may carry a panel with a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mobility {
    Fixed,
    Movable,
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

// The dot that shows a cursor on a panel: its place in dp from the panel's top left corner at
// the cursor's steps, and whether the trigger is down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dot {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) pressed: bool,
}

// A panel of the app and the states that its composition reads while it draws. A change of a
// state moves or draws the panel again without composing its content again.
#[derive(Clone, Copy)]
pub(crate) struct Panel {
    pub(crate) id: u64,
    // Where the panel's part of the atlas starts in dp, from the atlas's top left corner.
    pub(crate) origin: MutableState<[f32; 2]>,
    // The dot of the cursor on the panel, or `None` while no controller points at it.
    pub(crate) dot: MutableState<Option<Dot>>,
    // The surface of a surface panel once the backend has made it.
    pub(crate) surface: MutableState<Option<ExternalSurface>>,
}

// The panels of the app in a headset. The headset's backend provides it around the app's
// content, and the spatial composables register their panels in it. Outside a headset there
// is none, and the composables draw flat.
#[derive(Clone)]
pub(crate) struct Space {
    inner: Rc<Inner>,
}

struct Inner {
    entries: RefCell<Vec<Entry>>,
    next_id: Cell<u64>,
}

// A registered panel as the space keeps it.
struct Entry {
    slot: PanelSlot,
    panel: Panel,
    // What the panel's dot state holds, so that a comparison needs no read of the state.
    dot: Option<Dot>,
    // Whether the user carried the panel. Its pose then stays the user's.
    carried: bool,
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
    // A space with no panels.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                entries: RefCell::new(Vec::new()),
                next_id: Cell::new(1),
            }),
        }
    }

    // A new panel with its states. A composable calls it inside a `remember`, so that the states
    // end with the composable.
    pub(crate) fn new_panel(&self) -> Panel {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        Panel {
            id,
            origin: mutableStateOf([0.0, 0.0]),
            dot: mutableStateOf(None),
            surface: mutableStateOf(None),
        }
    }

    // Registers `panel` of `size` dp at `pose` and returns its part of the atlas. A new panel and
    // a panel of a new size stack every panel again.
    pub(crate) fn place(&self, panel: Panel, size: [f32; 2], pose: Pose) -> [f32; 4] {
        self.place_kind(panel, PanelKind::Content, size, pose)
    }

    // Registers the surface `panel` of `size` dp at `pose`. It takes no room in the atlas.
    pub(crate) fn place_surface(
        &self,
        panel: Panel,
        stereo_mode: StereoMode,
        size: [f32; 2],
        pose: Pose,
    ) {
        self.place_kind(panel, PanelKind::Surface(stereo_mode), size, pose);
    }

    fn place_kind(&self, panel: Panel, kind: PanelKind, size: [f32; 2], pose: Pose) -> [f32; 4] {
        let mut entries = self.inner.entries.borrow_mut();
        let (index, changed) = match entries.iter().position(|entry| entry.slot.id == panel.id) {
            Some(index) => {
                let entry = &mut entries[index];
                if !entry.carried {
                    entry.slot.pose = pose;
                }
                let slot = &mut entry.slot;
                let changed = slot.atlas[2..] != size;
                slot.atlas = [slot.atlas[0], slot.atlas[1], size[0], size[1]];
                (index, changed)
            }
            None => {
                entries.push(Entry {
                    slot: PanelSlot {
                        id: panel.id,
                        kind,
                        atlas: [0.0, 0.0, size[0], size[1]],
                        pose,
                        mobility: Mobility::Fixed,
                    },
                    panel,
                    dot: None,
                    carried: false,
                });
                (entries.len() - 1, true)
            }
        };
        if changed {
            restack(&mut entries);
        }
        entries[index].slot.atlas
    }

    // Sets whether the user may carry the panel `id`.
    pub(crate) fn set_mobility(&self, id: u64, mobility: Mobility) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.slot.mobility = mobility;
        }
    }

    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Puts the panel `id` at `pose`, which `place` keeps until the panel leaves.
    pub(crate) fn move_panel(&self, id: u64, pose: Pose) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.slot.pose = pose;
            entry.carried = true;
        }
    }

    #[cfg_attr(
        not(all(feature = "openxr", target_os = "android")),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Gives the surface panel `id` the surface that the backend made for it.
    pub(crate) fn publish_surface(&self, id: u64, surface: ExternalSurface) {
        let entries = self.inner.entries.borrow();
        if let Some(entry) = entries.iter().find(|entry| entry.slot.id == id) {
            entry.panel.surface.set(Some(surface));
        }
    }

    // Forgets the panel `id`. The panels below it move up.
    pub(crate) fn remove(&self, id: u64) {
        let mut entries = self.inner.entries.borrow_mut();
        entries.retain(|entry| entry.slot.id != id);
        restack(&mut entries);
    }

    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn panels(&self) -> Vec<PanelSlot> {
        self.inner
            .entries
            .borrow()
            .iter()
            .map(|entry| entry.slot)
            .collect()
    }

    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The width and height in dp that the atlas needs for every panel.
    pub(crate) fn atlas_size(&self) -> [f32; 2] {
        self.inner
            .entries
            .borrow()
            .iter()
            .filter(|entry| entry.slot.kind == PanelKind::Content)
            .fold([0.0, 0.0], |size, entry| {
                let [x, y, width, height] = entry.slot.atlas;
                [size[0].max(x + width), size[1].max(y + height)]
            })
    }

    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Shows the cursor on its panel and on no other. A panel draws again only when its dot
    // changes. A move below the dot's step changes nothing.
    pub(crate) fn set_cursor(&self, cursor: Option<Cursor>) {
        for entry in self.inner.entries.borrow_mut().iter_mut() {
            let dot = cursor
                .filter(|cursor| cursor.panel == entry.slot.id)
                .map(dot_of);
            if entry.dot != dot {
                entry.dot = dot;
                entry.panel.dot.set(dot);
            }
        }
    }
}

// Stacks the panels from the top of the atlas in the order they registered. A panel that moves
// gets its new origin in its state.
fn restack(entries: &mut [Entry]) {
    let mut top = 0.0;
    for entry in entries
        .iter_mut()
        .filter(|entry| entry.slot.kind == PanelKind::Content)
    {
        let [left, old_top, _, height] = entry.slot.atlas;
        // Tops lie on even dp, so a change is always more than half a dp
        if (old_top - top).abs() > 0.5 {
            entry.slot.atlas[1] = top;
            entry.panel.origin.set([left, top]);
        }
        // Tops on even dp stay on whole pixels at the densities of one and a half and two
        top = ((top + height + ATLAS_GAP) / 2.0).ceil() * 2.0;
    }
}

fn dot_of(cursor: Cursor) -> Dot {
    let step = |dp: f32| (dp / CURSOR_STEP).round() * CURSOR_STEP;
    Dot {
        x: step(cursor.x),
        y: step(cursor.y),
        pressed: cursor.pressed,
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
