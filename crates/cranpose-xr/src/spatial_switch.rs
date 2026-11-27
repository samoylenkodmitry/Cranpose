//! A switch whose body is a 3D rocker on a `SpatialTray`.

use cranpose::prelude::*;
use cranpose_core::{mutableStateOf, remember, rememberUpdatedState};

use crate::tray_part::{Part, PartShape, PartStates, TrayPart, local_tray};

/// The colors of a [`SpatialSwitch`], after Compose's `SwitchColors`: the rocker's face, and the
/// light on its end half that shines while the switch is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchColors {
    /// The rocker's face
    pub face: Color,
    /// The light that shines while the switch is on
    pub checked: Color,
}

impl Default for SwitchColors {
    fn default() -> Self {
        Self {
            face: Color(0.22, 0.26, 0.36, 1.0),
            checked: Color(0.35, 0.8, 1.0, 1.0),
        }
    }
}

/// A switch on a [`SpatialTray`](crate::SpatialTray), after Compose's `Switch`: a rocker whose end
/// half lies deeper while `checked` and whose start half lies deeper while not. A light on the
/// end half shines while it is on. A press anywhere on it flips it: `on_checked_change` gets the
/// new state at the release, and the rocker tilts over with a spring once the app sets `checked`.
/// The rocker sinks a little while pressed, and a ray that rests on it makes it glow. The press
/// gives the controller the haptic tick of every press on a panel. A disabled switch takes no
/// press. The switch draws no words: the app puts them on the panel over or beside it.
///
/// Jetpack XR has no 3D switch. Outside a tray, such as in a window, the switch is a flat track
/// with a knob at its end while checked and at its start while not.
#[composable]
pub fn SpatialSwitch(
    checked: bool,
    on_checked_change: impl Fn(bool) + 'static,
    modifier: Modifier,
    enabled: bool,
    colors: SwitchColors,
) {
    let source = rememberMutableInteractionSource();
    let flip = move |_| {
        if enabled {
            on_checked_change(!checked);
        }
    };
    let Some(tray) = local_tray().current() else {
        let track = if checked { colors.checked } else { colors.face };
        let alignment = if checked {
            Alignment::CENTER_END
        } else {
            Alignment::CENTER_START
        };
        Box(
            modifier
                .background(track)
                .rounded_corners(f32::MAX / 4.0)
                .padding(4.0)
                .clickable(flip),
            BoxSpec::default().content_alignment(alignment),
            || {
                Box(
                    Modifier::empty()
                        .size_points(24.0, 24.0)
                        .background(Color::WHITE)
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
        return;
    };
    let on = rememberUpdatedState(checked);
    let shades = rememberUpdatedState([colors.face, colors.checked]);
    let still = remember(|| mutableStateOf(0.0_f32)).with(|state| *state);
    let part = remember(|| {
        Part::new(
            PartShape::Rocker,
            PartStates {
                pressed: source.collectIsPressedAsState(),
                hovered: source.collectIsHoveredAsState(),
                on: on.as_state(),
                value: still.as_state(),
                colors: shades.as_state(),
            },
        )
    })
    .with(|part| *part);
    TrayPart(tray, part);
    Box(
        part.pressable(modifier.hoverable(source, enabled), source, flip),
        BoxSpec::default(),
        || {},
    );
}
