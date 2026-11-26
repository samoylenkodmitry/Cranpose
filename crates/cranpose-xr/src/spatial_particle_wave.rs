//! A wave of glowing points, such as the one beside a page's title.

use cranpose::prelude::*;

use crate::{
    spatial_choice_group::rememberPart,
    tray_part::{PartShape, TrayPart, local_tray},
};

/// A wave of glowing points on a [`SpatialHologram`](crate::SpatialHologram), such as beside a
/// page's title: a ridge of hills of points over `modifier`'s place, seen a little from above, that
/// fades out toward its right end. Bands of light run over it while the page shows. The wave takes
/// no press and draws nothing on the panel.
///
/// Jetpack XR has no such composable. Outside a hologram, the wave is an empty box of its place.
#[composable]
pub fn SpatialParticleWave(modifier: Modifier) {
    let Some(tray) = local_tray().current() else {
        Box(modifier, BoxSpec::default(), || {});
        return;
    };
    let source = rememberMutableInteractionSource();
    let part = rememberPart(PartShape::Wave, source, (true, 0.0));
    TrayPart(tray, part);
    Box(
        modifier.report_window_rect_state(part.rect),
        BoxSpec::default(),
        || {},
    );
}
