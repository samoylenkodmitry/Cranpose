use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_layout::Alignment;

use crate::material::{Glass, LiquidModifierExt};

/// A container rendered on the Liquid Glass material: its backdrop is
/// refracted/blurred behind `content`, clipped to `glass.shape`. `modifier`
/// applies to the whole surface, glass included, as a Compose modifier does:
/// its offset moves the glass and its padding is space around it.
///
/// ```ignore
/// GlassSurface(Modifier::empty().fill_max_width(), Glass::regular(), || {
///     Text("On glass", Modifier::empty().padding(12.0), body_style);
/// });
/// ```
#[composable]
pub fn GlassSurface(modifier: Modifier, glass: Glass, content: impl FnMut() + 'static) {
    let modifier = modifier.glass_effect(glass);
    Box(
        modifier,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        content,
    );
}
