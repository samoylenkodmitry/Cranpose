//! A gem that takes a press: an octahedron of glowing glass on a hologram or a tray.

use cranpose::prelude::*;
use cranpose_core::{remember, rememberUpdatedState};

use crate::tray_part::{Part, PartShape, PartStates, TrayPart, local_tray};

/// How the glow of a [`SpatialGem`] behaves over time. Jetpack XR has no gem and so no such glow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GemGlow {
    /// The glow holds still
    #[default]
    Steady,
    /// The glow swells and fades once each 1.6 s, as a light that says that something runs
    Pulse,
}

/// A gem on a [`SpatialHologram`](crate::SpatialHologram) that takes a press: an octahedron of
/// glowing glass, two square pyramids base to base, as wide as the smaller side of `modifier`'s
/// place. It turns slowly about its upright axis while it shows. `color` is the color of its
/// glass, and `glow` says whether its glow holds still or pulses. A ray that rests on the gem
/// makes it glow brighter. A press sinks it and makes it flash, and it gives the controller the
/// haptic tick of every press on a panel. `on_click` runs at the release, as a click on any panel
/// does. A disabled gem takes no hover and no click.
///
/// `interaction_source` receives the gem's hover and press, as the `interactionSource` of
/// Compose's `Button` does, so the app can read them with `collectIsHoveredAsState` and pick
/// `color` by them, such as a color that says what a press would do.
///
/// On a [`SpatialTray`](crate::SpatialTray) the gem is a satin dome, as a ball button is. Jetpack
/// XR has no gem. Outside a hologram and a tray, such as in a window, the gem is a flat diamond of
/// `color`.
#[composable]
pub fn SpatialGem(
    on_click: impl Fn() + 'static,
    modifier: Modifier,
    enabled: bool,
    color: Color,
    glow: GemGlow,
    interaction_source: MutableInteractionSource,
) {
    let click = move |_| {
        if enabled {
            on_click();
        }
    };
    let Some(tray) = local_tray().current() else {
        Canvas(
            modifier
                .hoverable(interaction_source, enabled)
                .clickable(click),
            move |scope| {
                let size = scope.size();
                let side = size.width.min(size.height);
                let (x, y) = (size.width / 2.0, size.height / 2.0);
                let mut path = Path::new();
                path.move_to(Point::new(x, y - side / 2.0));
                path.line_to(Point::new(x + side / 2.0, y));
                path.line_to(Point::new(x, y + side / 2.0));
                path.line_to(Point::new(x - side / 2.0, y));
                path.close();
                scope.draw_path(&path, Brush::solid(color), DrawStyle::Fill);
            },
        );
        return;
    };
    let colors = rememberUpdatedState([color, color]);
    let pulsing = rememberUpdatedState(glow == GemGlow::Pulse);
    let still = rememberUpdatedState(0.0_f32);
    let part = remember(|| {
        Part::new(
            PartShape::Gem,
            PartStates {
                pressed: interaction_source.collectIsPressedAsState(),
                hovered: interaction_source.collectIsHoveredAsState(),
                on: pulsing.as_state(),
                value: still.as_state(),
                colors: colors.as_state(),
            },
        )
    })
    .with(|part| *part);
    TrayPart(tray, part);
    Box(
        part.pressable(
            modifier.hoverable(interaction_source, enabled),
            interaction_source,
            click,
        ),
        BoxSpec::default(),
        || {},
    );
}
