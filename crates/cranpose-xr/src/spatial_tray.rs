//! A panel of 2D content that stands on a tray of 3D geometry.

use std::{cell::RefCell, rc::Rc};

use cranpose::{prelude::*, space::Light};
use cranpose_animation::{Easing, animate_float_as_state_with_initial, tween};
use cranpose_core::{CompositionLocalProvider, derivedStateOf, remember, rememberUpdatedState};

use crate::{
    HologramColors, HologramStage, SubspaceModifier,
    hologram_scene::{HOLOGRAM_HIDE_MILLIS, HOLOGRAM_SHOW_MILLIS, HologramScene, hologram_extent},
    space::{Space, local_space},
    spatial_panel::PlacedPanel,
    spatial_volume::PlacedVolume,
    subspace_modifier::DEFAULT_PANEL_SIZE,
    tray_part::{TrayParts, local_tray},
    tray_scene::{TRAY_LIGHT, TrayScene},
    volumes::VolumeDraws,
};

// How long a tray takes to show and to hide, in milliseconds
const SHOW_MILLIS: u64 = 180;
const HIDE_MILLIS: u64 = 140;
// The depth of the tray's box: its parts stand behind the panel's plane, and the box is centered
// on that plane
const TRAY_DEPTH: f32 = 100.0;
// How far a tray's controls reach out of its box as they rise and lift, in dp
const TRAY_REACH: f32 = 24.0;
// The radius of a flat tray's corners, as the base's corners in a headset
const FLAT_CORNER: f32 = 24.0;

/// A panel of 2D content that stands on a tray of 3D geometry: a base with depth, light and
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
/// compose nothing. The panel's layer takes the compositor's depth test as any panel does, so a
/// controller's model or another tray in front of the words covers them. `base` is the base's
/// color, or `None` for controls that stand on no base, such as a single ball. Controls on no base
/// cast no shadow, so a tray of them adds no shadow map to the frame.
///
/// The modifier's pose, `movable`, `layer_priority` and following subspace work as on a
/// [`SpatialPanel`](crate::SpatialPanel). The tray moves with its panel when the user carries it
/// and when its subspace follows a target. The tray is one panel layer of the compositor and the
/// meshes of the scene's one projection layer.
///
/// `visible` shows and hides the tray as Compose's `AnimatedVisibility` does: the tray rises from
/// behind its place in 180 ms and grows to its size, and it sinks back and shrinks in 140 ms. The
/// words show with the start of the show and leave with the start of the hide, so the panel draws
/// once for each. Once the tray has hidden, it composes neither its panel nor its meshes, so a
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
    ShownTray(modifier, visible, TrayLook::Solid(base), content);
}

// How a tray draws its base and its controls' bodies.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TrayLook {
    // Satin bodies on a base of the color, if any, in a light with soft shadows
    Solid(Option<Color>),
    // Glowing glass with no base, and the stage around it
    Hologram(HologramColors, HologramStage),
}

impl TrayLook {
    // How long the tray takes to show and to hide, in milliseconds.
    fn millis(self) -> (u64, u64) {
        match self {
            Self::Solid(_) => (SHOW_MILLIS, HIDE_MILLIS),
            Self::Hologram(..) => (HOLOGRAM_SHOW_MILLIS, HOLOGRAM_HIDE_MILLIS),
        }
    }

    // What the tray's volume draws: a hologram's objects add light and leave no depth. A tray's
    // controls rise and lift a little out of its box, and a hologram's stage stands around it.
    fn draws(self, size: [f32; 2]) -> VolumeDraws {
        match self {
            Self::Solid(_) => VolumeDraws {
                writes_depth: true,
                extent: Some([
                    size[0] / 2.0 + TRAY_REACH,
                    size[1] / 2.0 + TRAY_REACH,
                    TRAY_DEPTH / 2.0 + TRAY_REACH,
                ]),
            },
            Self::Hologram(_, stage) => VolumeDraws {
                writes_depth: false,
                extent: Some(hologram_extent(size, stage)),
            },
        }
    }

    // The background of the content outside a headset.
    fn flat_background(self) -> Color {
        match self {
            Self::Solid(Some(base)) => base,
            Self::Solid(None) | Self::Hologram(..) => Color(0.0, 0.0, 0.0, 0.0),
        }
    }
}

// A tray of `look` that shows while `visible` and composes nothing once it has hidden.
#[composable]
pub(crate) fn ShownTray(
    modifier: SubspaceModifier,
    visible: bool,
    look: TrayLook,
    content: impl FnMut() + 'static,
) {
    let (show, hide) = look.millis();
    let animation = tween(if visible { show } else { hide }, Easing::EaseOut);
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
        Some(space) => PlacedTray(space, modifier, (size, look), (visible, shown), content),
        None => {
            Box(
                Modifier::empty()
                    .size_points(size[0], size[1])
                    .background(look.flat_background())
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
    (size, look): ([f32; 2], TrayLook),
    (visible, shown): (bool, State<f32>),
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
                    .graphics_layer_value(GraphicsLayer {
                        alpha: f32::from(u8::from(visible)),
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
        (
            [size[0], size[1], TRAY_DEPTH],
            Some(panel),
            look.draws(size),
        ),
        move |scope| match look {
            TrayLook::Solid(base) => {
                // Controls on no base have nothing to cast a shadow on, so their light casts none
                // and the frame draws no shadow map for them
                scope.light(Light {
                    shadows: base.is_some(),
                    ..TRAY_LIGHT
                });
                TrayScene(tray, base, size);
            }
            // A hologram's objects glow and take no light
            TrayLook::Hologram(colors, stage) => HologramScene(tray, (colors, stage), size),
        },
    );
}
