//! With little memory left, the room growth left past a raster scene's lists.
//! A screen keeps its scene, and a `Vec` that doubled to take one more item
//! keeps room for as many again, which the heap never gets back.

use cranpose_core::collections::growth_slack::GrowthSlack;

use crate::{draw::ClipBounds, raster_scene::RasterScene};

// Gives back the room past the items of each list of `scene` and of
// `reach`, the list of where each item reaches. Out of line: a frame with
// ample memory only tests the headroom it reads anyway.
#[inline(never)]
pub(crate) fn trim(scene: &mut RasterScene, reach: &mut Vec<Option<ClipBounds>>) {
    scene.shapes.trim_growth_slack();
    scene.images.trim_growth_slack();
    scene.texts.trim_growth_slack();
    scene.shadows.trim_growth_slack();
    scene.order.trim_growth_slack();
    scene.subtrees.trim_growth_slack();
    scene.passed_down.trim_growth_slack();
    reach.trim_growth_slack();
}
