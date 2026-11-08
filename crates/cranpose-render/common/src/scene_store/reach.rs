// Where a record's subtree draws: its reach, worked out when a reader asks
// for the records a change left stale, and its extent in its parent's space.

use cranpose_core::RecordId;
use cranpose_ui::Rect;
use cranpose_ui_graphics::{expand_rect, primitive_coverage_rect};

use super::{ItemRef, LayerRef, RecordFlags, SceneStore};
use crate::graph::{DrawPrimitiveNode, DrawRunNode};

/// Where a record's subtree draws, in one space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reach {
    /// It draws nothing.
    Empty,
    /// It draws within the rect.
    Finite(Rect),
    /// It may draw anywhere: paint whose bounds are not known, an elevation
    /// shadow, whose reach depends on the window's light, or a projection
    /// that reaches the eye plane.
    Unknown,
}

impl Reach {
    /// The reach of `rect`; `None` reads as unknown.
    pub fn of(rect: Option<Rect>) -> Self {
        rect.map_or(Self::Unknown, Self::Finite)
    }

    /// Where either draws.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Empty, reach) | (reach, Self::Empty) => reach,
            (Self::Finite(a), Self::Finite(b)) => Self::Finite(a.union(b)),
        }
    }

    /// The part `clip` leaves; `None` cuts nothing.
    #[must_use]
    pub fn within(self, clip: Option<Rect>) -> Self {
        let Some(clip) = clip else {
            return self;
        };
        match self {
            Self::Empty => Self::Empty,
            Self::Finite(rect) => rect.intersect(clip).map_or(Self::Empty, Self::Finite),
            Self::Unknown => Self::Finite(clip),
        }
    }

    /// The reach with its rect mapped by `map`; a rect mapped to a value
    /// that is not finite reads as unknown.
    #[must_use]
    pub fn mapped(self, map: impl FnOnce(Rect) -> Rect) -> Self {
        let Self::Finite(rect) = self else {
            return self;
        };
        let mapped = map(rect);
        if [mapped.x, mapped.y, mapped.width, mapped.height]
            .iter()
            .all(|value| value.is_finite())
        {
            Self::Finite(mapped)
        } else {
            Self::Unknown
        }
    }

    /// The rect, when the reach is finite.
    pub fn rect(self) -> Option<Rect> {
        match self {
            Self::Finite(rect) => Some(rect),
            Self::Empty | Self::Unknown => None,
        }
    }
}

/// What a record draws in its parent's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    /// Where its subtree draws, placed by its transform and grown by its
    /// effects, beside its elevation shadow.
    pub reach: Reach,
    /// What casts its elevation shadow, whose reach depends on the
    /// window's light; `None` without one.
    pub caster: Option<ShadowCaster>,
}

/// A record's elevation shadow, in its parent's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowCaster {
    /// The record's bounds, placed by its transform.
    pub bounds: Rect,
    /// The elevation, scaled by the record's own scale.
    pub height: f32,
}

// Works out the reach of `id` when a change left it stale, after that of
// each of its children a change left stale.
pub(super) fn refresh(store: &mut SceneStore, id: RecordId) {
    let index = id.index();
    if !store.tree[index].flags.contains(RecordFlags::REACH_STALE) {
        return;
    }
    let span = store.tree[index].items;
    for position in 0..span.len() {
        if let Some(child) = store.items.span(span)[position].as_child() {
            refresh(store, child);
        }
    }
    let reach = items_reach(LayerRef::new(store, id));
    store.set_reach(index, reach);
    let flags = &mut store.tree[index].flags;
    *flags = flags.without(RecordFlags::REACH_STALE);
}

// The reach of what `layer`'s items draw, within its clip.
fn items_reach(layer: LayerRef<'_>) -> Reach {
    let clip = layer.visual_clip_rect();
    let mut reach = Reach::Empty;
    for item in layer.items() {
        reach = reach.union(match item {
            ItemRef::Draw(draw) => draw_reach(draw),
            ItemRef::Text(text) => Reach::of(text.draw_bounds()),
            ItemRef::Run(run) => run_reach(run),
            ItemRef::Layer(child) | ItemRef::Leaf(child) => {
                let extent = child.extent();
                match extent.caster {
                    Some(_) => Reach::Unknown,
                    None => extent.reach,
                }
            }
        });
        if reach == Reach::Unknown && clip.is_none() {
            break;
        }
    }
    reach.within(clip)
}

fn draw_reach(draw: &DrawPrimitiveNode) -> Reach {
    Reach::of(primitive_coverage_rect(&draw.primitive)).within(draw.clip)
}

fn run_reach(run: &DrawRunNode) -> Reach {
    if run.summary.has_shadow {
        return Reach::Unknown;
    }
    run.recording.bounds().map_or(Reach::Empty, Reach::Finite)
}

// The extent of `layer` in its parent's space.
pub(super) fn extent(layer: LayerRef<'_>) -> Extent {
    let graphics_layer = layer.graphics_layer();
    let to_parent = layer.transform_to_parent();
    let effect = layer.effect().map_or(0.0, |effect| {
        effect.input_padding() + effect.output_padding()
    });
    let backdrop = layer.backdrop().map_or(Reach::Empty, |backdrop| {
        Reach::Finite(expand_rect(layer.local_bounds(), backdrop.output_padding()))
    });
    let space = graphics_layer.space.is_some();
    let reach = if space {
        Reach::Unknown
    } else {
        layer
            .reach()
            .mapped(|rect| expand_rect(rect, effect))
            .union(backdrop)
            .mapped(|rect| to_parent.bounds_for_rect(rect))
    };
    let caster = (graphics_layer.shadow_elevation > 0.0).then(|| ShadowCaster {
        bounds: to_parent.bounds_for_rect(layer.local_bounds()),
        height: crate::layer_shadow::shadow_height(graphics_layer),
    });
    Extent { reach, caster }
}
