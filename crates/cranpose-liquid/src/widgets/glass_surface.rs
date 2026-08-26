//! The primitive glass container.

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_layout::Alignment;

use crate::material::{Glass, LiquidModifierExt};

/// A container rendered on the Liquid Glass material: its backdrop is
/// refracted/blurred behind `content`, clipped to `glass.shape`.
///
/// ```ignore
/// GlassSurface(Modifier::empty().padding(12.0), Glass::regular(), || {
///     Text("On glass", Modifier::empty(), body_style);
/// });
/// ```
#[composable]
#[allow(non_snake_case)]
pub fn GlassSurface(modifier: Modifier, glass: Glass, content: impl FnMut() + 'static) {
    let modifier = Modifier::empty().glass_effect(glass).then(modifier);
    Box(
        modifier,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        content,
    );
}
