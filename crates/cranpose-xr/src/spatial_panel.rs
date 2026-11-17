//! A flat panel of 2D content in a subspace.

use cranpose::prelude::*;

use crate::{
    SubspaceModifier,
    space::{Dot, SUBSPACE_ORIGIN, Space, local_space},
    subspace_modifier::DEFAULT_PANEL_SIZE,
};

// The diameter of the dot that shows where a controller points, in dp.
const CURSOR_DOT: f32 = 12.0;

/// A flat panel of ordinary Cranpose content, `modifier` sized and placed in its subspace.
/// Jetpack XR's `SpatialPanel`.
///
/// In a headset the panel is a layer of the headset's compositor, which samples the panel's
/// pixels once at the display's resolution, so its text stays sharp. A controller that points
/// at it moves the pointer over its content, and its trigger presses. Outside a headset the
/// panel draws as a box of its size.
#[composable]
pub fn SpatialPanel(modifier: SubspaceModifier, content: impl FnMut() + 'static) {
    let size = modifier.size_or(DEFAULT_PANEL_SIZE);
    match local_space().current() {
        Some(space) => PlacedPanel(space, modifier, size, content),
        None => {
            Box(
                Modifier::empty().size_points(size[0], size[1]),
                BoxSpec::default(),
                content,
            );
        }
    }
}

#[composable]
fn PlacedPanel(
    space: Space,
    modifier: SubspaceModifier,
    size: [f32; 2],
    content: impl FnMut() + 'static,
) {
    let panel = remember({
        let space = space.clone();
        move || space.new_panel()
    })
    .with(|panel| *panel);
    space.place(panel, size, SUBSPACE_ORIGIN.then(&modifier.pose()));
    let id = panel.id;
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove(id))
    });
    // The offset reads the origin while the scene draws, so a panel that moves in the atlas does
    // not compose its content again.
    Box(
        Modifier::empty()
            .absolute_offset_with(move || {
                let [x, y] = panel.origin.get();
                Point::new(x, y)
            })
            .required_size(Size::new(size[0], size[1]))
            .clip_to_bounds(),
        BoxSpec::default(),
        move || {
            content();
            CursorDot(panel.dot);
        },
    );
}

// The dot reads the cursor's state, so a cursor that moves composes only the dot.
#[composable]
fn CursorDot(dot: MutableState<Option<Dot>>) {
    let Some(dot) = dot.get() else {
        return;
    };
    let color = if dot.pressed {
        Color(0.55, 0.75, 1.0, 1.0)
    } else {
        Color(1.0, 1.0, 1.0, 0.95)
    };
    Box(
        Modifier::empty()
            .offset(dot.x - CURSOR_DOT / 2.0, dot.y - CURSOR_DOT / 2.0)
            .size_points(CURSOR_DOT, CURSOR_DOT)
            .background(color)
            .rounded_corners(CURSOR_DOT / 2.0)
            .border(
                1.5,
                Color(0.1, 0.1, 0.12, 0.9),
                RoundedCornerShape::uniform(CURSOR_DOT / 2.0),
            ),
        BoxSpec::default(),
        || {},
    );
}
