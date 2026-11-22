//! A panel of 2D content that stands on a tray of 3D geometry.

use std::{cell::RefCell, rc::Rc};

use cranpose::prelude::*;
use cranpose_animation::{Easing, animate_float_as_state_with_initial, tween};
use cranpose_core::{CompositionLocalProvider, derivedStateOf, remember, rememberUpdatedState};

use crate::{
    SubspaceModifier,
    space::{Space, local_space},
    spatial_panel::PlacedPanel,
    spatial_volume::PlacedVolume,
    subspace_modifier::DEFAULT_PANEL_SIZE,
    tray_part::{TrayParts, local_tray},
    tray_scene::{TRAY_LIGHT, TrayScene},
};

// How long a tray takes to show and to hide, in milliseconds
const SHOW_MILLIS: u64 = 180;
const HIDE_MILLIS: u64 = 140;
// The depth of the tray's box: its parts stand behind the panel's plane, and the box is centered
// on that plane
const TRAY_DEPTH: f32 = 100.0;
// The radius of a flat tray's corners, as the base's corners in a headset
const FLAT_CORNER: f32 = 24.0;

/// A panel of 2D content that stands on a tray of 3D geometry: a base with depth, light and soft
/// shadows, with the 3D bodies of the [`SpatialButton`](crate::SpatialButton)s,
/// [`SpatialSwitch`](crate::SpatialSwitch)es and [`SpatialSlider`](crate::SpatialSlider)s of the
/// content on it. Jetpack XR has no such composable: its controls are flat, on a
/// [`SpatialPanel`](crate::SpatialPanel). Its Material components for XR take the `Spatial`
/// prefix, as these do.
///
/// The content is ordinary composables. They lay out and take input on a panel of the size that
/// `modifier` gives, which shows at the compositor's full resolution, so the words of the content
/// stay sharp. The panel draws no background. The tray's meshes stand behind the panel's plane in
/// the 3D scene of the volumes, which draws them for each eye. Each control's body stands where the
/// control lays out on the panel. A key sinks 5 mm while a ray presses it and rises at the release.
/// A switch flips, and a slider's knob slides along its groove. A ray that rests on a control lifts
/// it a little and makes it glow. The meshes read these animations while the scene draws, so they
/// compose nothing. Where the compositor tests the panels against the scene's depth, the words show
/// over the parts. `base` is the base's color, or `None` for controls that stand on no base, such as
/// a single ball.
///
/// The modifier's pose, `movable`, `layer_priority` and following subspace work as on a
/// [`SpatialPanel`](crate::SpatialPanel). The tray moves with its panel when the user carries it
/// and when its subspace follows a target. The tray is one panel layer of the compositor and the
/// meshes of the scene's one projection layer.
///
/// `visible` shows and hides the tray as Compose's `AnimatedVisibility` does: the tray rises from
/// behind its place in 180 ms and grows to its size as its words fade in, and it sinks back and
/// fades out in 140 ms. Once it has hidden, it composes neither its panel nor its meshes, so a
/// hidden tray costs nothing. Its content's state ends then.
///
/// Outside a headset the tray is a box of the panel's size with a background of `base` and rounded
/// corners, and its controls draw flat.
#[composable]
pub fn SpatialTray(
    modifier: SubspaceModifier,
    visible: bool,
    base: Option<Color>,
    content: impl FnMut() + 'static,
) {
    let animation = tween(
        if visible { SHOW_MILLIS } else { HIDE_MILLIS },
        Easing::EaseOut,
    );
    let shown =
        animate_float_as_state_with_initial(0.0, f32::from(u8::from(visible)), animation, "tray");
    let wanted = rememberUpdatedState(visible);
    // The tray composes while it shows, and while it hides until the hide ends
    let gone = derivedStateOf(move || !wanted.get() && shown.get() <= 0.0);
    if gone.get() {
        return;
    }
    let size = modifier.size_or(DEFAULT_PANEL_SIZE);
    match local_space().current() {
        Some(space) => PlacedTray(space, modifier, (size, base), shown, content),
        None => {
            let background = base.unwrap_or(Color(0.0, 0.0, 0.0, 0.0));
            Box(
                Modifier::empty()
                    .size_points(size[0], size[1])
                    .background(background)
                    .rounded_corners(FLAT_CORNER),
                BoxSpec::default(),
                content,
            );
        }
    }
}

#[composable]
fn PlacedTray(
    space: Space,
    modifier: SubspaceModifier,
    (size, base): ([f32; 2], Option<Color>),
    shown: State<f32>,
    content: impl FnMut() + 'static,
) {
    let tray = remember(|| TrayParts::new(shown)).with(|tray| *tray);
    // The content composes inside two closures of the panel, which share it
    let content = Rc::new(RefCell::new(content));
    let pose = modifier.pose();
    let panel = PlacedPanel(space.clone(), modifier, size, move || {
        let content = Rc::clone(&content);
        CompositionLocalProvider([local_tray().provides(Some(tray))], move || {
            let content = Rc::clone(&content);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .report_window_rect_state(tray.root)
                    .graphics_layer(move || GraphicsLayer {
                        alpha: tray.shown.get(),
                        ..GraphicsLayer::default()
                    }),
                BoxSpec::default(),
                move || (content.borrow_mut())(),
            );
        });
    });
    PlacedVolume(
        space,
        SubspaceModifier::new()
            .size(size[0], size[1])
            .with_pose(pose),
        ([size[0], size[1], TRAY_DEPTH], Some(panel)),
        move |scope| {
            scope.light(TRAY_LIGHT);
            TrayScene(tray, base, size);
        },
    );
}
