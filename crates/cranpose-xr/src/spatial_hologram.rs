//! A panel of 2D content whose controls float as glowing glass objects.

use cranpose::prelude::*;

use crate::{
    HologramColors, HologramStage, SubspaceModifier,
    spatial_tray::{ShownTray, TrayLook},
};

/// A panel of 2D content whose controls float in front of the user as objects of glowing glass,
/// with no panel behind them: a hologram. Jetpack XR has no such composable, no material that
/// glows by the angle to the eye, no glow that hides nothing and no field of sparks that draws as
/// one mesh.
///
/// The content is ordinary composables, as on a [`SpatialTray`](crate::SpatialTray): they lay out
/// and take input on a panel of the size that `modifier` gives, which draws no background and shows
/// its words at the compositor's full resolution. Each control draws its body as a hologram
/// object where it lays out: a [`SpatialChoiceGroup`](crate::SpatialChoiceGroup) an object for each
/// option, a [`SpatialButton`](crate::SpatialButton) a key of glass or a crystal, a
/// [`SpatialSwitch`](crate::SpatialSwitch) a capsule whose lit half slides to the chosen side, a
/// [`SpatialSlider`](crate::SpatialSlider) a groove with a crystal knob, a
/// [`SpatialEmblem`](crate::SpatialEmblem) a round emblem with rings that orbit it and a
/// [`SpatialRibbon`](crate::SpatialRibbon) a band of glass behind words.
///
/// Every object adds light to what lies behind it, with a bright rim where its surface turns from
/// the eye, glowing edges and a faint fill, in `colors`. The objects hide nothing and leave no
/// depth, so the panel's words always show over them and no order among them matters. A
/// controller's model in front of the words still hides them. A chosen option grows and glows
/// brighter, a pressed object sinks and flashes, and a ray that rests on one makes it brighter.
/// No full-screen pass draws a bloom: the glow is the objects' own light and round halos that fade
/// to their outline.
///
/// `stage` adds a projector under the page with a cone of light up to it, bands of glass that wind
/// about upright lines beside it and sparks that drift beside it. The sparks are one mesh that
/// draws at once. Nothing of the stage stands over the controls. While the page shows, its rings
/// orbit, its crystals and chosen objects turn and its ribbons turn about their lines, each frame. A hologram with no stage and no such objects, such as a single crystal button, draws
/// only when something changes.
///
/// `visible` shows and hides the page as Compose's `AnimatedVisibility` does: the objects rise out
/// of the projector and grow to their places in 420 ms, and they sink back into it in 260 ms. The
/// words show with the start of the show and leave with the start of the hide. Once the page has
/// hidden, it composes neither its panel nor its objects and its clock stops, so a hidden page
/// costs nothing. Its content's state ends then.
///
/// The modifier's pose, `movable`, `layer_priority` and following subspace work as on a
/// [`SpatialPanel`](crate::SpatialPanel). Outside a headset the hologram is a box of the panel's
/// size with no background, and its controls draw flat.
#[composable]
pub fn SpatialHologram(
    modifier: SubspaceModifier,
    visible: bool,
    colors: HologramColors,
    stage: HologramStage,
    content: impl FnMut() + 'static,
) {
    ShownTray(
        modifier,
        visible,
        TrayLook::Hologram(colors, stage),
        content,
    );
}
