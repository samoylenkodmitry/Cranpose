//! A band of glass behind words.

use cranpose::prelude::*;

use crate::{
    spatial_choice_group::rememberPart,
    tray_part::{PartShape, TrayPart, local_tray},
};

/// A band of glass on a [`SpatialHologram`](crate::SpatialHologram) behind `content`, such as a
/// page's title: a sheet of `modifier`'s place, bent away from the viewer at its ends, with glowing
/// edges and a faint fill. The ribbon takes no press.
///
/// Jetpack XR has no such composable. Outside a hologram and a tray, the ribbon is its content alone.
#[composable]
pub fn SpatialRibbon(modifier: Modifier, content: impl FnMut() + 'static) {
    let start = BoxSpec::default().content_alignment(Alignment::CENTER_START);
    let Some(tray) = local_tray().current() else {
        Box(modifier, start, content);
        return;
    };
    let source = rememberMutableInteractionSource();
    let part = rememberPart(PartShape::Ribbon, source, (true, 0.0));
    TrayPart(tray, part);
    Box(modifier.report_window_rect_state(part.rect), start, content);
}
