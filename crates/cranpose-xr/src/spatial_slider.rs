//! A slider whose body is a 3D groove and knob on a `SpatialTray`.

use std::rc::Rc;

use cranpose::prelude::*;
use cranpose_core::{SideEffect, rememberMutableStateOf, rememberUpdatedState};

use crate::{
    tray_part::{Part, PartShape, PartStates, TrayPart, local_tray},
    tray_scene::KNOB,
};

/// The colors of a [`SpatialSlider`], after Compose's `SliderColors`: the knob, and the part of
/// the groove from its start to the knob.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderColors {
    /// The knob, and the marks of the values
    pub thumb: Color,
    /// The groove from its start to the knob
    pub active_track: Color,
}

impl Default for SliderColors {
    fn default() -> Self {
        Self {
            thumb: Color(0.85, 0.88, 0.95, 1.0),
            active_track: Color(0.35, 0.8, 1.0, 1.0),
        }
    }
}

/// A slider on a [`SpatialTray`](crate::SpatialTray), after Compose's `Slider`: a knob that a ray
/// drags along a groove, with a fill from the groove's start to the knob. `value` runs from 0 at
/// the start to 1 at the end. With `steps` above zero the slider takes `steps + 2` values spread
/// evenly from 0 to 1, as Compose's `steps` gives, and the groove shows a mark under each.
/// `on_value_change` gets each new value while the ray drags, snapped to the nearest of them, and
/// `on_value_change_finished` runs at the release. A press on the groove moves the knob there. The
/// knob sinks a little while held, and glides to each new value. A ray that rests on the slider
/// makes the knob glow. The knob is 32 dp wide, so the slider needs at least 38 dp of height.
///
/// Jetpack XR has no 3D slider. Outside a tray, such as in a window, the slider is a flat track
/// and knob.
#[composable]
pub fn SpatialSlider(
    value: f32,
    on_value_change: impl Fn(f32) + 'static,
    modifier: Modifier,
    enabled: bool,
    steps: u32,
    on_value_change_finished: impl Fn() + 'static,
    colors: SliderColors,
) {
    let value = value.clamp(0.0, 1.0);
    let current = rememberUpdatedState(value);
    let stops = steps.saturating_add(2);
    let on_value_change = Rc::new(on_value_change);
    let change = move |raw: f32| {
        let snapped = snap(raw, steps, stops);
        if enabled && (snapped - current.get_non_reactive()).abs() > f32::EPSILON {
            on_value_change(snapped);
        }
    };
    let finished = move || {
        if enabled {
            on_value_change_finished();
        }
    };
    let spec = SliderSpec::default().thumb_extent(KNOB).enabled(enabled);
    let Some(tray) = local_tray().current() else {
        Slider(
            modifier,
            value,
            change,
            finished,
            spec,
            move |scope: SliderScope| {
                FlatTrack(scope.thumb_offset(), colors);
            },
        );
        return;
    };
    let source = rememberMutableInteractionSource();
    let dragging = rememberMutableStateOf(|| false);
    let shades = rememberUpdatedState([colors.thumb, colors.active_track]);
    let held = rememberUpdatedState(true);
    let part = remember(|| {
        Part::new(
            PartShape::Slider {
                stops: if steps == 0 { 0 } else { stops },
            },
            PartStates {
                pressed: dragging.as_state(),
                hovered: source.collectIsHoveredAsState(),
                on: held.as_state(),
                value: current.as_state(),
                colors: shades.as_state(),
            },
        )
    })
    .with(|part| *part);
    TrayPart(tray, part);
    Slider(
        modifier
            .report_window_rect_state(part.rect)
            .hoverable(source, enabled),
        value,
        change,
        finished,
        spec,
        move |scope: SliderScope| {
            // The knob's body reads whether a ray holds it
            let now = scope.is_dragging();
            SideEffect(move || {
                if dragging.get_non_reactive() != now {
                    dragging.set(now);
                }
            });
        },
    );
}

// `value` at the nearest of `stops` values spread evenly from 0 to 1, or as it is with no steps.
fn snap(value: f32, steps: u32, stops: u32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if steps == 0 {
        return value;
    }
    let last = (stops - 1) as f32;
    (value * last).round() / last
}

// The track and the knob of a slider outside a tray.
#[composable]
fn FlatTrack(offset: f32, colors: SliderColors) {
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default().content_alignment(Alignment::CENTER_START),
        move || {
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(6.0)
                    .background(colors.active_track)
                    .rounded_corners(3.0),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .offset(offset, 0.0)
                    .size_points(KNOB, KNOB)
                    .background(colors.thumb)
                    .rounded_corners(KNOB / 2.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}
