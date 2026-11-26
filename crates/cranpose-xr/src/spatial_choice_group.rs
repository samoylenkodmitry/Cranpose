//! A choice of one option of several, whose options are each a 3D object.

use std::rc::Rc;

use cranpose::prelude::*;
use cranpose_core::{key, mutableStateOf, remember, rememberUpdatedState};

use crate::tray_part::{Part, PartShape, PartStates, TrayPart, TrayParts, local_tray};

// What an option of an index runs: the group's select or its words. Two are equal when they are
// the same closure.
#[derive(Clone)]
pub(crate) struct Action(Rc<dyn Fn(usize)>);

impl Action {
    fn run(&self, index: usize) {
        (self.0)(index);
    }
}

impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// The height under a cube, a flower or the arc's tick that holds an option's words, in dp
pub(crate) const WORDS_UNDER: f32 = 26.0;
// The share of a stepper's width that each arrow takes
const ARROW_SHARE: f32 = 0.2;

/// How a [`SpatialChoiceGroup`] draws its options: each form shows the option's value by its shape.
///
/// Jetpack XR has no choice of 3D objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChoiceForm {
    /// Cubes of glass that grow from the first option to the last, such as sizes, with a finer
    /// grid on the faces of a later option. The words stand under each cube.
    Cubes,
    /// Faceted gems of one size. The chosen gem is larger, turns and stands in a beam of light
    /// from below. The words stand under each gem.
    Gems,
    /// Flowers whose petals grow in number and length from the first option, a closed bud, to the
    /// last, such as levels from off to high. The words stand under each flower.
    Petals,
    /// An arc of light with a tick at each option and a crystal knob that floats over the chosen
    /// one and slides to a new choice, such as rates. The words stand under the ticks.
    Arc,
    /// The chosen option's words under a crystal with rings that orbit it, between an arrow to the
    /// option before and an arrow to the option after. An arrow with no option to go to dims and
    /// takes no press.
    Stepper,
}

impl ChoiceForm {
    // Where an option's words stand in its place.
    fn words(self) -> Alignment {
        Alignment::BOTTOM_CENTER
    }
}

/// A choice of one of `count` options, after Compose's `SingleChoiceSegmentedButtonRow` with a
/// `SegmentedButton` for each option: each option is a 3D object of `form` on a
/// [`SpatialHologram`](crate::SpatialHologram) or a [`SpatialTray`](crate::SpatialTray).
/// `selected` is the option that holds now, or `None`. A press on an option's place runs
/// `on_select` with the option's index at the release, as a click on any panel does; the app then
/// sets `selected`, and the chosen object blooms. `label` draws the words of the option of an index
/// on the panel, at the place that `form` gives them. The options share the group's width
/// equally, so the object of each stands over the middle of its share.
///
/// A [`ChoiceForm::Stepper`] shows only the chosen option, with its words in the middle, and an
/// arrow on each side: a press on an arrow selects the option before or after the chosen one.
///
/// Outside a hologram and a tray, such as in a window, the group is a row of flat boxes, the chosen
/// one filled, or for a stepper the chosen option's words between two arrow boxes.
#[composable]
pub fn SpatialChoiceGroup(
    selected: Option<usize>,
    on_select: impl Fn(usize) + 'static,
    count: usize,
    form: ChoiceForm,
    modifier: Modifier,
    label: impl Fn(usize) + 'static,
) {
    let on_select = Action(Rc::new(on_select));
    let label = Action(Rc::new(label));
    let tray = local_tray().current();
    if form == ChoiceForm::Stepper {
        Stepper(tray, (selected, count), modifier, (on_select, label));
        return;
    }
    let group = move || {
        let (on_select, label) = (on_select.clone(), label.clone());
        Row(
            Modifier::empty().fill_max_size(),
            RowSpec::default(),
            move || {
                for index in 0..count {
                    let (on_select, label) = (on_select.clone(), label.clone());
                    let chosen = selected == Some(index);
                    key((index, count, form), move || {
                        ChoiceOption(tray, (form, index, count), chosen, (on_select, label));
                    });
                }
            },
        );
    };
    match tray.filter(|_| form == ChoiceForm::Arc) {
        Some(tray) => {
            Box(modifier, BoxSpec::default(), move || {
                ArcTrack(tray, selected, count);
                group();
            });
        }
        None => {
            Box(modifier, BoxSpec::default(), group);
        }
    }
}

// One option: its place takes the press and holds its words, and its object stands on the tray.
#[composable]
fn ChoiceOption(
    tray: Option<TrayParts>,
    (form, index, count): (ChoiceForm, usize, usize),
    chosen: bool,
    (on_select, label): (Action, Action),
) {
    let place = Modifier::empty().weight(1.0).fill_max_height();
    let words = BoxSpec::default().content_alignment(form.words());
    let click = move |_| on_select.run(index);
    let Some(tray) = tray else {
        let fill = if chosen {
            Color(0.25, 0.45, 0.9, 1.0)
        } else {
            Color(0.0, 0.0, 0.0, 0.0)
        };
        Box(
            place
                .background(fill)
                .rounded_corners(10.0)
                .clickable(click),
            words,
            move || label.run(index),
        );
        return;
    };
    let source = rememberMutableInteractionSource();
    let shape = PartShape::Choice {
        form,
        index: u32::try_from(index).unwrap_or(u32::MAX),
        count: u32::try_from(count).unwrap_or(u32::MAX),
    };
    let part = rememberPart(shape, source, (chosen, 0.0));
    TrayPart(tray, part);
    Box(
        part.pressable(place.hoverable(source, true), source, click),
        words,
        move || label.run(index),
    );
}

// The arc behind the options of `ChoiceForm::Arc`: its value is the chosen option's place along it,
// from 0 at the first to 1 at the last.
#[composable]
fn ArcTrack(tray: TrayParts, selected: Option<usize>, count: usize) {
    let last = count.saturating_sub(1).max(1);
    let value = selected.map_or(0.0, |index| index.min(last) as f32 / last as f32);
    let source = rememberMutableInteractionSource();
    let shape = PartShape::Track {
        count: u32::try_from(count).unwrap_or(u32::MAX),
    };
    let part = key(count, || {
        rememberPart(shape, source, (selected.is_some(), value))
    });
    TrayPart(tray, part);
    Box(
        Modifier::empty()
            .fill_max_size()
            .report_window_rect_state(part.rect),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Stepper(
    tray: Option<TrayParts>,
    (selected, count): (Option<usize>, usize),
    modifier: Modifier,
    (on_select, label): (Action, Action),
) {
    let before = selected.and_then(|index| index.checked_sub(1));
    let after = selected.map(|index| index + 1).filter(|&next| next < count);
    Row(modifier, RowSpec::default(), move || {
        let to = on_select.clone();
        StepArrow(tray, false, before.map(move |index| (index, to)));
        let label = label.clone();
        let middle = Modifier::empty().weight(1.0).fill_max_height();
        let under = BoxSpec::default().content_alignment(ChoiceForm::Stepper.words());
        let words = move || {
            if let Some(index) = selected {
                label.run(index);
            }
        };
        match tray {
            Some(tray) => {
                let source = rememberMutableInteractionSource();
                let part = rememberPart(PartShape::Crystal, source, (true, 0.0));
                TrayPart(tray, part);
                Box(middle.report_window_rect_state(part.rect), under, words);
            }
            None => {
                Box(middle, under, words);
            }
        }
        let to = on_select.clone();
        StepArrow(tray, true, after.map(move |index| (index, to)));
    });
}

// An arrow of a stepper toward the option after or before the chosen one: `to` is that option and
// what selects it, or `None` where the arrow has no option to go to.
#[composable]
fn StepArrow(tray: Option<TrayParts>, forward: bool, to: Option<(usize, Action)>) {
    let enabled = to.is_some();
    let click = move |_| {
        if let Some((index, select)) = &to {
            select.run(*index);
        }
    };
    let place = Modifier::empty().weight(ARROW_SHARE).fill_max_height();
    let Some(tray) = tray else {
        let words = if forward { ">" } else { "<" };
        Box(
            place.clickable(click),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                Text(words, Modifier::empty(), TextStyle::default());
            },
        );
        return;
    };
    let source = rememberMutableInteractionSource();
    let part = rememberPart(PartShape::Arrow { forward }, source, (enabled, 0.0));
    TrayPart(tray, part);
    Box(
        part.pressable(place.hoverable(source, enabled), source, click),
        BoxSpec::default(),
        || {},
    );
}

// A part of `shape` whose press and hover `source` shows, which is on while `on` and holds `value`.
#[composable]
pub(crate) fn rememberPart(
    shape: PartShape,
    source: MutableInteractionSource,
    (on, value): (bool, f32),
) -> Part {
    let on = rememberUpdatedState(on);
    let value = rememberUpdatedState(value);
    let colors = remember(|| mutableStateOf([Color::WHITE; 2])).with(|state| *state);
    remember(|| {
        Part::new(
            shape,
            PartStates {
                pressed: source.collectIsPressedAsState(),
                hovered: source.collectIsHoveredAsState(),
                on: on.as_state(),
                value: value.as_state(),
                colors: colors.as_state(),
            },
        )
    })
    .with(|part| *part)
}
