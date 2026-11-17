//! The 3D area of an app.

use cranpose::prelude::*;

use crate::space::local_space;

// The space between panels that a subspace shows flat, in dp.
const FLAT_SPACING: f32 = 24.0;

/// The 3D area of an app, which holds its spatial panels. Jetpack XR's `Subspace`.
///
/// In a headset its panels stand in the room, a meter in front of where the viewer's head was
/// when the app started. Outside a headset it shows them flat, side by side.
#[composable]
pub fn Subspace(content: impl FnMut() + 'static) {
    if local_space().current().is_some() {
        Box(
            Modifier::empty().fill_max_size(),
            BoxSpec::default(),
            content,
        );
    } else {
        Row(
            Modifier::empty(),
            RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(FLAT_SPACING)),
            content,
        );
    }
}
