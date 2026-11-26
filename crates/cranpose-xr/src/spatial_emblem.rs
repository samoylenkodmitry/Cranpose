//! A round emblem with rings that orbit it.

use cranpose::prelude::*;

use crate::{
    spatial_choice_group::rememberPart,
    tray_part::{PartShape, TrayPart, local_tray},
};

/// A round emblem on a [`SpatialHologram`](crate::SpatialHologram), such as the sign of a page: a
/// disc of glowing rings as wide as the smaller side of `modifier`'s place, with two rings that
/// orbit it while the page shows. `content` draws on the panel over the disc, such as a symbol.
/// The emblem takes no press.
///
/// Jetpack XR has no such composable. Outside a hologram and a tray, the emblem is its content alone.
#[composable]
pub fn SpatialEmblem(modifier: Modifier, content: impl FnMut() + 'static) {
    let centered = BoxSpec::default().content_alignment(Alignment::CENTER);
    let Some(tray) = local_tray().current() else {
        Box(modifier, centered, content);
        return;
    };
    let source = rememberMutableInteractionSource();
    let part = rememberPart(PartShape::Emblem, source, (true, 0.0));
    TrayPart(tray, part);
    Box(
        modifier.report_window_rect_state(part.rect),
        centered,
        content,
    );
}
