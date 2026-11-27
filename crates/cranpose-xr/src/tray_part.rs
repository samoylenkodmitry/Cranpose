//! A control's 3D body on a tray, and the list of them that a tray draws.
//!
//! A control on a `SpatialTray` lays out and takes input on the tray's panel, where it draws only
//! its words. Its body is a part: the states that the tray's scene reads to place and move the
//! control's meshes behind the panel. The scene reads them while it draws, so a press, a hover or a
//! drag moves the meshes with no composition of the panel.

use std::sync::atomic::{AtomicU64, Ordering};

use cranpose::{Color, Indication, Modifier, MutableInteractionSource, Point, Rect};
use cranpose_core::{
    CompositionLocal, DisposableEffect, DisposableEffectResult, MutableState, State, mutableStateOf,
};

use crate::{ChoiceForm, locals::with_locals};

// What a part looks like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PartShape {
    // A key cap that sinks while pressed
    Key,
    // A ball that sinks while pressed
    Dome,
    // A rocker that tilts to its end while on and to its start while off
    Rocker,
    // A groove with a knob, and a mark for each of `stops` values
    Slider {
        stops: u32,
    },
    // The option numbered `index` of `count` of a choice group, as an object of `form`, which
    // shines while chosen
    Choice {
        form: ChoiceForm,
        index: u32,
        count: u32,
    },
    // The track of a choice group of `ChoiceForm::Arc`: a tick at each of its `count` options and
    // a knob at its value
    Track {
        count: u32,
    },
    // The crystal that holds a stepper's value, with rings that orbit it
    Crystal,
    // A stepper's arrow toward the next option, or toward the one before; it dims while it has
    // no option to go to
    Arrow {
        forward: bool,
    },
    // A round emblem with rings that orbit it
    Emblem,
    // A band of glass behind words, such as a title
    Ribbon,
    // A wave of glowing points
    Wave,
    // A gem that turns, and whose glow swells and fades while `on`
    Gem,
}

impl PartShape {
    // Whether a hologram turns or orbits the part with time while it shows.
    pub(crate) fn moves(self) -> bool {
        matches!(
            self,
            Self::Choice { .. }
                | Self::Track { .. }
                | Self::Crystal
                | Self::Emblem
                | Self::Wave
                | Self::Gem
        )
    }
}

// A control's body.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Part {
    pub(crate) id: u64,
    pub(crate) shape: PartShape,
    // Where the control lays out, in the window's logical pixels
    pub(crate) rect: MutableState<Rect>,
    pub(crate) pressed: State<bool>,
    pub(crate) hovered: State<bool>,
    // A rocker's state, or whether a slider's value is the app's
    pub(crate) on: State<bool>,
    // A slider's value, from 0 to 1
    pub(crate) value: State<f32>,
    // The face's color, and the color of what is on: a rocker's light, a slider's fill and knob
    pub(crate) colors: State<[Color; 2]>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

// The states of a part that the control keeps up to date.
#[derive(Clone, Copy)]
pub(crate) struct PartStates {
    pub(crate) pressed: State<bool>,
    pub(crate) hovered: State<bool>,
    pub(crate) on: State<bool>,
    pub(crate) value: State<f32>,
    pub(crate) colors: State<[Color; 2]>,
}

impl Part {
    // A part of `shape` whose control keeps `states`.
    pub(crate) fn new(shape: PartShape, states: PartStates) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            shape,
            rect: mutableStateOf(Rect::default()),
            pressed: states.pressed,
            hovered: states.hovered,
            on: states.on,
            value: states.value,
            colors: states.colors,
        }
    }

    // `modifier` for the control whose body this is: it reports where the control lays out, shows
    // the pointer's press to the body through `source` and runs `click` at a click. The control
    // draws no indication, because its body shows the hover and the press.
    pub(crate) fn pressable(
        &self,
        modifier: Modifier,
        source: MutableInteractionSource,
        click: impl Fn(Point) + 'static,
    ) -> Modifier {
        modifier
            .report_window_rect_state(self.rect)
            .press_interaction_source(source)
            .clickable_with_indication(Indication::NONE, click)
    }
}

// The parts on one tray, and where the tray's panel content lays out.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct TrayParts {
    pub(crate) list: MutableState<Vec<Part>>,
    // The panel content's place in the window, which the parts' places count from
    pub(crate) root: MutableState<Rect>,
    // How far the tray has come into view, from 0 to 1, as its show and hide animate
    pub(crate) shown: State<f32>,
}

impl TrayParts {
    pub(crate) fn new(shown: State<f32>) -> Self {
        Self {
            list: mutableStateOf(Vec::new()),
            root: mutableStateOf(Rect::default()),
            shown,
        }
    }

    fn add(&self, part: Part) {
        self.list.update(|list| {
            if !list.iter().any(|known| known.id == part.id) {
                list.push(part);
            }
        });
    }

    fn remove(&self, id: u64) {
        self.list.update(|list| list.retain(|part| part.id != id));
    }

    // Where `rect` of the window lies on a tray of `size` dp, as the offset of its center from the
    // tray's center: x to the right and y down.
    pub(crate) fn center_of(&self, rect: Rect, size: [f32; 2]) -> [f32; 2] {
        let root = self.root.get();
        [
            rect.x - root.x + rect.width / 2.0 - size[0] / 2.0,
            rect.y - root.y + rect.height / 2.0 - size[1] / 2.0,
        ]
    }
}

// The parts of the tray whose panel holds the content, or `None` outside a tray.
pub(crate) fn local_tray() -> CompositionLocal<Option<TrayParts>> {
    with_locals(|locals| locals.tray.clone())
}

// Keeps `part` on `tray` while the calling control stays in the composition.
#[cranpose::composable]
pub(crate) fn TrayPart(tray: TrayParts, part: Part) {
    DisposableEffect(part.id, move |_| {
        tray.add(part);
        DisposableEffectResult::new(move || tray.remove(part.id))
    });
}
