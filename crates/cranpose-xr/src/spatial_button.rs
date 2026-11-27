//! A button whose body is a 3D key cap or ball on a `SpatialTray`.

use cranpose::prelude::*;
use cranpose_core::{mutableStateOf, remember, rememberUpdatedState};

use crate::tray_part::{Part, PartShape, PartStates, TrayPart, local_tray};

// The radius of a flat key's corners
const FLAT_CORNER: f32 = 12.0;

/// The shape of a [`SpatialButton`]'s body. Compose's `Button` takes a `Shape` of its outline:
/// a 3D body has no such shape, so this enum names the two bodies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonShape {
    /// A key cap with rounded corners and a rounded front edge
    #[default]
    Key,
    /// A ball as wide as the smaller side of the button
    Ball,
}

/// A button on a [`SpatialTray`](crate::SpatialTray), after Compose's `Button`: a key cap or a
/// ball of `color` that sinks 5 mm while a ray presses it and rises at the release. A ray that
/// rests on it lifts it a little and makes it glow. The press gives the controller the haptic tick
/// of every press on a panel. `on_click` runs at the release, as a click on any panel does.
/// `content` draws on the tray's panel, over the body, such as the button's words. A disabled
/// button takes no hover and no click.
///
/// Jetpack XR has no 3D button. Outside a tray, such as in a window, the button is a flat box of
/// `color` with rounded corners.
#[composable]
pub fn SpatialButton(
    on_click: impl Fn() + 'static,
    modifier: Modifier,
    enabled: bool,
    shape: ButtonShape,
    color: Color,
    content: impl FnMut() + 'static,
) {
    let source = rememberMutableInteractionSource();
    let click = move |_| {
        if enabled {
            on_click();
        }
    };
    let centered = BoxSpec::default().content_alignment(Alignment::CENTER);
    let Some(tray) = local_tray().current() else {
        let corner = match shape {
            ButtonShape::Key => FLAT_CORNER,
            ButtonShape::Ball => f32::MAX / 4.0,
        };
        Box(
            modifier
                .background(color)
                .rounded_corners(corner)
                .clickable(click),
            centered,
            content,
        );
        return;
    };
    let colors = rememberUpdatedState([color, color]);
    let off = remember(|| mutableStateOf(false)).with(|state| *state);
    let still = remember(|| mutableStateOf(0.0_f32)).with(|state| *state);
    let body = match shape {
        ButtonShape::Key => PartShape::Key,
        ButtonShape::Ball => PartShape::Dome,
    };
    let part = remember(|| {
        Part::new(
            body,
            PartStates {
                pressed: source.collectIsPressedAsState(),
                hovered: source.collectIsHoveredAsState(),
                on: off.as_state(),
                value: still.as_state(),
                colors: colors.as_state(),
            },
        )
    })
    .with(|part| *part);
    TrayPart(tray, part);
    Box(
        part.pressable(modifier.hoverable(source, enabled), source, click),
        centered,
        content,
    );
}
