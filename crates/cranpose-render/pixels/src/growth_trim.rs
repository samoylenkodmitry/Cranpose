//! The room growth left past a raster scene's lists, with little memory left
//! or once the scene lost most of its records. A screen keeps its scene, and
//! a `Vec` that doubled to take one more item keeps room for as many again,
//! which the heap never gets back.

use cranpose_core::collections::growth_slack::GrowthSlack;

use crate::Picture;

// The fewest records a scene keeps room for, whatever it shrank to.
const MIN_RECORDS: usize = 64;

// Whether `picture`'s scene keeps room for over four times its records: it
// lost most of them, and the room of its peak would stay while the screen
// holds still. A scene whose records swing within a factor of two keeps its
// room.
#[inline]
pub(crate) fn shrank(picture: &Picture) -> bool {
    picture
        .raster
        .as_ref()
        .is_some_and(|scene| scene.subtrees.capacity() > 4 * scene.subtrees.len().max(MIN_RECORDS))
}

// Gives back the room past the items of each list of `picture`'s scene and
// of the list of where each item reaches. Out of line: a frame with ample
// memory only tests the headroom it reads anyway.
#[inline(never)]
pub(crate) fn trim(picture: &mut Picture) {
    let Some(scene) = picture.raster.as_mut() else {
        return;
    };
    scene.shapes.trim_growth_slack();
    scene.images.trim_growth_slack();
    scene.texts.trim_growth_slack();
    scene.shadows.trim_growth_slack();
    scene.order.trim_growth_slack();
    scene.subtrees.trim_growth_slack();
    scene.passed_down.trim_growth_slack();
    picture.reach.trim_growth_slack();
}
