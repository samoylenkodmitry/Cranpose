//! A glTF model in a subspace.

use std::{rc::Rc, sync::Arc};

use cranpose::{
    Alignment, Box, BoxSpec, Color, ContentScale, Image, LaunchedEffect, Modifier, Size,
    composable,
    space::{Light, Mat4, Material, MeshSurface, MeshSurfaces, Vec3},
};
use cranpose_core::key;

use crate::{
    SpatialVolume, SpatialVolumeScope, SubspaceModifier,
    gltf_animation::GltfAnimationState,
    gltf_fit::ModelFit,
    gltf_model::LayerContent,
    spatial_gltf_model_state::{LoadedModel, SpatialGltfModelState},
};

// The longest step that an animation takes in one frame, in seconds, so that a late frame does not
// jump the animation.
const LONGEST_STEP: f32 = 0.1;

// The light on a model until the content sets another: from the upper left over the viewer's
// shoulder, in the volume's axes.
const DEFAULT_LIGHT: Light = Light {
    direction: Vec3::new(0.45, 0.6, 0.65),
    ambient: 0.35,
    shadows: false,
    size: 0.0,
};

/// A glTF 2.0 model in a box of the size that `modifier` gives, centered at the modifier's pose in
/// its subspace. Jetpack XR's `SpatialGltfModel`. It needs the `gltf` feature.
///
/// The model loads on a worker thread, as `state` says (see [`rememberSpatialGltfModelState`](crate::rememberSpatialGltfModelState)).
/// While it loads, and if it fails, the composable shows nothing, as Jetpack XR's does. The
/// [`SpatialGltfModelState::status`] tells which. Once the model has loaded the composable is a
/// [`SpatialVolume`] whose content is the model.
///
/// # Size
///
/// The model keeps its proportions, as in Jetpack XR. It scales up or down, by the smallest ratio
/// of a side that `modifier` sets ([`width`](SubspaceModifier::width),
/// [`height`](SubspaceModifier::height) and [`depth`](SubspaceModifier::depth)) to the model's own
/// side, so it touches the box on one side and lies inside it on the others. A side that the
/// modifier leaves out is the model's scaled side. With no size set, the model has its own size,
/// which is a meter for each glTF unit, 1000 dp. The model's middle lies at the box's middle.
/// The bounding box is the one of the model at rest, before any animation moves it.
///
/// # What shows
///
/// The scene shows the node tree of the file's default scene, each mesh with its node's
/// transform and its material's base color: the base color texture, multiplied by the base color
/// factor. Alpha modes `OPAQUE`, `MASK` and `BLEND` work as the format says. A texture wider or
/// higher than 2048 pixels is halved until it fits. Everything else in a glTF material is left
/// out: metallic, roughness, normal, occlusion and emissive values, texture transforms, and the
/// sampler, so a texture clamps at its edges and never repeats. The scene draws every surface
/// double sided. Skins, morph targets, cameras and lights of the file, vertex colors, and
/// primitives that are not triangles are left out; a skinned mesh stands in its bind pose. Where
/// a primitive has no normals, the vertices take the mean direction of their triangles, where the
/// format asks for flat shading. Jetpack XR's renderer takes the whole material, and lights
/// the model with image based lighting.
///
/// The scene has one directional light for all volumes. The model sets a default light, from the
/// upper left. The `content` callback runs in the model's volume, so it can set another with
/// [`light`](SpatialVolumeScope::light), take the pointer events of a ray, and add meshes of its
/// own in the volume's space: its origin is the box's center, x to the right, y down and z away
/// from the viewer, and a unit is a dp. Jetpack XR's `content` holds spatial composables that
/// stand at the model's nodes. Cranpose has no counterpart of the nodes, so the content's meshes
/// stand at the box.
///
/// # Animation
///
/// [`SpatialGltfModelState::animations`] lists the animations of the file once it has loaded.
/// Each has `start`, `stop`, `pause` and `resume`, a loop and a speed, and drives the translation,
/// rotation and scale of its nodes. A frame that has no playing animation takes no time for them.
///
/// ```ignore
/// // A source made from bytes is a new source each time, so the composable keeps it
/// let source = remember(|| SpatialGltfModelSource::from_bytes(MODEL)).with(Clone::clone);
/// let state = rememberSpatialGltfModelState(source);
/// SpatialGltfModel(
///     state.clone(),
///     SubspaceModifier::new().size(300.0, 300.0).depth(300.0),
///     |_| {},
/// );
/// // The animations exist once the model has loaded. An effect keyed by the status starts
/// // them once, where a call in the composable's body would start them again at each
/// // composition.
/// let status = state.status();
/// LaunchedEffect(status.clone(), async move |_| {
///     if status == SpatialGltfModelStatus::Loaded {
///         for animation in state.animations() {
///             animation.set_looping(true);
///             animation.start();
///         }
///     }
/// });
/// ```
#[composable]
pub fn SpatialGltfModel(
    state: SpatialGltfModelState,
    modifier: SubspaceModifier,
    content: impl FnMut(&SpatialVolumeScope) + 'static,
) {
    let Some(loaded) = state.loaded() else {
        return;
    };
    ModelVolume(loaded, modifier, content);
}

#[composable]
fn ModelVolume(
    loaded: Rc<LoadedModel>,
    modifier: SubspaceModifier,
    content: impl FnMut(&SpatialVolumeScope) + 'static,
) {
    let fit = ModelFit::new(&loaded.model, modifier.size_limits());
    ModelAnimations(Rc::clone(&loaded));
    let mut content = content;
    SpatialVolume(
        modifier.size(fit.size[0], fit.size[1]).depth(fit.size[2]),
        move |scope| {
            scope.light(DEFAULT_LIGHT);
            ModelLayers(Rc::clone(&loaded), fit.root);
            content(scope);
        },
    );
}

// Runs the animations while one plays. A frame with none costs nothing: no effect runs, so the
// model draws no more often than it did.
#[composable]
fn ModelAnimations(loaded: Rc<LoadedModel>) {
    if loaded.animations.is_empty() {
        return;
    }
    let playing = loaded
        .animations
        .iter()
        .any(|animation| animation.animation_state() == GltfAnimationState::Playing);
    let requests = loaded.requests.get();
    LaunchedEffect(
        (playing, requests, Rc::clone(&loaded)),
        async move |scope| {
            // A change of what plays poses the nodes once, which covers a stop and the end of a run
            loaded.pose();
            if !playing {
                return;
            }
            let mut last = scope.with_frame_nanos(|now| now).await;
            loop {
                let now = scope.with_frame_nanos(|now| now).await;
                let step = (now.saturating_sub(last) as f32 * 1.0e-9).min(LONGEST_STEP);
                last = now;
                for animation in &loaded.animations {
                    animation.advance(step);
                }
                loaded.pose();
            }
        },
    );
}

// One node for each material of the model: the picture of the material, drawn on every mesh that
// the material covers.
#[composable]
fn ModelLayers(loaded: Rc<LoadedModel>, fit: Mat4) {
    for layer in 0..loaded.layers.len() {
        let loaded = Rc::clone(&loaded);
        key(layer, move || ModelLayer(loaded, layer, fit));
    }
}

#[composable]
fn ModelLayer(loaded: Rc<LoadedModel>, layer: usize, fit: Mat4) {
    let picture = loaded.model.layers[layer].clone();
    let size = Size::new(picture.size[0] as f32, picture.size[1] as f32);
    let modifier = Modifier::empty()
        .required_size(size)
        .mesh_surfaces(move || {
            let draws = &loaded.layers[layer];
            loaded.read_worlds(|worlds| {
                let surface = |draw: usize| {
                    let draw = &loaded.model.draws[draw];
                    MeshSurface {
                        mesh: Arc::clone(&draw.mesh),
                        transform: worlds[draw.node].then(&fit),
                        material: Material::Lit,
                        casts_shadow: false,
                        solid: true,
                    }
                };
                MeshSurfaces::new(
                    surface(draws.owner),
                    draws.more.iter().map(|&draw| surface(draw)).collect(),
                )
            })
        });
    match picture.content {
        LayerContent::Color([red, green, blue, alpha]) => {
            Box(
                modifier.background(Color(red, green, blue, alpha)),
                BoxSpec::default(),
                || {},
            );
        }
        LayerContent::Bitmap(bitmap) => {
            Box(modifier, BoxSpec::default(), move || {
                Image(
                    bitmap.clone(),
                    None,
                    Modifier::empty().fill_max_size(),
                    Alignment::CENTER,
                    ContentScale::FillBounds,
                    1.0,
                    None,
                );
            });
        }
    }
}
