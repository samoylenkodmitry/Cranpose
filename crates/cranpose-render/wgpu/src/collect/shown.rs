// The areas a window's walk records of the records it draws in place, and
// of those beneath a surface that composites each pixel where it lies: the
// frontend keeps them per target and works each frame's damage out of them.

use cranpose_core::RecordRef;
use cranpose_render_common::{
    graph::ProjectiveTransform,
    primitive_emit::resolve_clip,
    scene_store::{LayerRef, Reach},
};
use cranpose_ui_graphics::{Point, Rect};

use super::{LayerScene, WalkContext};
use crate::{
    rrect_shadow::rrect_shadow_bounds,
    scene::CompositorScene,
    target_state::{Cull, Visit},
};

// Where a walk that records how it draws each record lies in the space of
// the walk's root: the origin of its space there, and what of the root's
// space the clips above it leave.
#[derive(Clone, Copy)]
pub(super) struct ShownSpace {
    origin: Point,
    clip: Option<Rect>,
}

impl ShownSpace {
    // The space of the walk of a root.
    pub(super) const ROOT: Self = Self {
        origin: Point { x: 0.0, y: 0.0 },
        clip: None,
    };

    // `reach`, in the walk's space, in the root's, within the clips above
    // the walk.
    fn place(self, reach: Reach) -> Reach {
        reach
            .mapped(|rect| rect.translate(self.origin.x, self.origin.y))
            .within(self.clip)
    }
}

// Where the walk of `layer`'s content, which `transform` places in the
// space of the walk of `context` and draws into a surface of its own,
// records how it draws each record: the surface composites each of its
// pixels where it lies, as the layer only moves and draws no effect. `None`
// when the walk of `context` records nothing, or a change beneath `layer`
// changes all its surface writes.
pub(super) fn surface_space(
    layer: LayerRef<'_>,
    transform: ProjectiveTransform,
    context: &WalkContext<'_>,
) -> Option<ShownSpace> {
    let space = context.settings.shown?;
    let origin = transform
        .as_translation()
        .filter(|_| !context.settings.in_space && layer.effect().is_none())?;
    let clip = context
        .visual_clip
        .map(|clip| clip.translate(space.origin.x, space.origin.y));
    Some(ShownSpace {
        origin: Point::new(space.origin.x + origin.x, space.origin.y + origin.y),
        clip: resolve_clip(space.clip, clip),
    })
}

// How a walk drew a record's subtree, in the space of the walk's root.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ShownArea {
    // In place, or through a surface that composites each pixel where it
    // lies, within the rect.
    Drawn(Rect),
    // Through its own surface, composited within the rect, with no areas of
    // what lies beneath it: a change there changes all of it.
    Composite(Rect),
    // Not at all.
    Hidden,
    // Within bounds the walk does not know.
    Unknown,
}

// A record, how a walk drew it, and where it drew its own draws.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShownRecord {
    pub(crate) record: RecordRef,
    pub(crate) area: ShownArea,
    pub(crate) own: ShownArea,
    // Where the origin of the record's space lies in the walk root's, when
    // the walk drew the record in place and only moved its space there.
    pub(crate) origin: Option<Point>,
}

// The cull `layer`'s subtree walks under, from `cull`, its parent's;
// `None` when the walk leaves `layer` out.
#[inline]
pub(super) fn culled<'s>(
    layer: LayerRef<'_>,
    cull: Option<&'s Cull<'s>>,
) -> Option<Option<&'s Cull<'s>>> {
    match cull {
        Some(cull) => visit(layer, cull),
        None => Some(None),
    }
}

#[inline(never)]
fn visit<'s>(layer: LayerRef<'_>, cull: &'s Cull<'s>) -> Option<Option<&'s Cull<'s>>> {
    match cull.visit(layer.store().record_ref(layer.id())) {
        Visit::Skip => None,
        Visit::Walk => Some(Some(cull)),
        Visit::WalkAll => Some(None),
    }
}

// Records that the walk drew nothing of `layer`.
#[inline]
pub(super) fn record_hidden(layer: LayerRef<'_>, context: &WalkContext<'_>, out: &mut LayerScene) {
    if context.settings.shown.is_some() && !context.settings.in_space {
        out.shown.push(ShownRecord {
            record: layer.store().record_ref(layer.id()),
            area: ShownArea::Hidden,
            own: ShownArea::Hidden,
            origin: None,
        });
    }
}

// Records that the walk drew `layer`, placed at the walk's offset, in place
// or as a composite whose content the walk keeps no areas of, with the
// shadows it pushed from `shadows_from` on.
#[inline]
pub(super) fn record_drawn(
    layer: LayerRef<'_>,
    context: &WalkContext<'_>,
    drawn: (bool, usize),
    out: &mut LayerScene,
) {
    if let Some(space) = context.settings.shown
        && !context.settings.in_space
    {
        record_drawn_area(layer, (context, space), drawn, out);
    }
}

#[inline(never)]
fn record_drawn_area(
    layer: LayerRef<'_>,
    (context, space): (&WalkContext<'_>, ShownSpace),
    (composite, shadows_from): (bool, usize),
    out: &mut LayerScene,
) {
    let offset = context.offset;
    let reach = layer
        .extent()
        .reach
        .mapped(|rect| rect.translate(offset.x, offset.y))
        .union(pushed_shadows(
            &out.scene,
            shadows_from,
            context.settings.raster_scale.nominal(),
        ))
        .within(context.visual_clip);
    let area = shown_area(space.place(reach), composite);
    let (own, origin) = if composite {
        (area, None)
    } else {
        let to_parent = layer.transform_to_parent();
        let own = layer
            .own_reach()
            .mapped(|rect| {
                to_parent
                    .bounds_for_rect(rect)
                    .translate(offset.x, offset.y)
            })
            .within(context.visual_clip);
        let origin = to_parent.as_translation().map(|origin| {
            Point::new(
                origin.x + offset.x + space.origin.x,
                origin.y + offset.y + space.origin.y,
            )
        });
        (shown_area(space.place(own), false), origin)
    };
    out.shown.push(ShownRecord {
        record: layer.store().record_ref(layer.id()),
        area,
        own,
        origin,
    });
}

fn shown_area(reach: Reach, composite: bool) -> ShownArea {
    match reach {
        Reach::Empty => ShownArea::Hidden,
        Reach::Finite(rect) if composite => ShownArea::Composite(rect),
        Reach::Finite(rect) => ShownArea::Drawn(rect),
        Reach::Unknown => ShownArea::Unknown,
    }
}

// Where the rounded-rect shadows pushed from `from` on draw.
fn pushed_shadows(scene: &CompositorScene, from: usize, scale: f32) -> Reach {
    scene.rrect_shadows[from..]
        .iter()
        .map(|shadow| match rrect_shadow_bounds(shadow, scale) {
            Some(bounds) => Reach::Finite(bounds),
            None => Reach::Empty,
        })
        .fold(Reach::Empty, Reach::union)
}
