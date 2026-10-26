use cranpose_ui_graphics::Rect;

use super::{Item, PassSegment, composite_visible, op_is_visible_in_rect};
use crate::{
    render::{run_draw_bounds, text_draw_geometry_in_rect},
    run_store::run_has_shapes,
    scene::{CompositorScene, DrawOp, DrawOpKind, RunDraw, TextDraw},
};

pub(super) fn merge_items<'a>(
    segment: &PassSegment<'a>,
    viewport_rect: Rect,
    target_size: (u32, u32),
    skip_text: bool,
) -> impl Iterator<Item = Item<'a>> + use<'a> {
    let scene = segment.scene;
    let root_scale = segment.scale;
    let mut ops = segment.ops.iter().enumerate().peekable();
    let mut composites = segment.composites.iter().peekable();
    let mut pending = OpItems::empty(viewport_rect, root_scale);
    let offset = segment.offset;
    let scissor = segment.scissor;
    let first_run_window = segment.first_run_window.clone();
    std::iter::from_fn(move || {
        loop {
            if let Some(item) = pending.next() {
                return Some(item);
            }
            let next_z = ops.peek().map(|(_, op)| op.z_index);
            if composites
                .peek()
                .is_some_and(|composite| next_z.is_none_or(|z| composite.z_index <= z))
            {
                let composite = composites.next().expect("peeked composite");
                if composite_visible(composite, target_size, offset, scissor) {
                    return Some(Item::Composite(composite));
                }
                continue;
            }
            let (op_index, op) = ops.next()?;
            let window = (op_index == 0).then(|| first_run_window.clone()).flatten();
            pending = OpItems::of(scene, op, window, (viewport_rect, root_scale, skip_text));
        }
    })
}

/// What one op draws into a segment's viewport.
enum OpItem<'a> {
    Draw(Item<'a>),
    /// A shadow's texts, drawn next, and its casters when unblurred.
    Shadow(std::slice::Iter<'a, TextDraw>, Option<Item<'a>>),
    Nothing,
}

/// The items one op draws into a segment's viewport, in order: its own, or
/// a shadow's casters and then the shadow's texts that reach the viewport.
pub(super) struct OpItems<'a> {
    first: Option<Item<'a>>,
    texts: std::slice::Iter<'a, TextDraw>,
    viewport_rect: Rect,
    root_scale: f32,
}

impl<'a> OpItems<'a> {
    fn empty(viewport_rect: Rect, root_scale: f32) -> Self {
        Self {
            first: None,
            texts: [].iter(),
            viewport_rect,
            root_scale,
        }
    }

    /// See [`op_item`].
    pub(super) fn of(
        scene: &'a CompositorScene,
        op: &DrawOp,
        window: Option<std::ops::Range<u32>>,
        (viewport_rect, root_scale, skip_text): (Rect, f32, bool),
    ) -> Self {
        let (first, texts) =
            match op_item(scene, op, window, (viewport_rect, root_scale, skip_text)) {
                OpItem::Draw(item) => (Some(item), [].iter()),
                OpItem::Shadow(texts, casters) => (casters, texts),
                OpItem::Nothing => (None, [].iter()),
            };
        Self {
            first,
            texts,
            viewport_rect,
            root_scale,
        }
    }
}

impl<'a> Iterator for OpItems<'a> {
    type Item = Item<'a>;

    fn next(&mut self) -> Option<Item<'a>> {
        if let Some(item) = self.first.take() {
            return Some(item);
        }
        let (viewport_rect, root_scale) = (self.viewport_rect, self.root_scale);
        self.texts.find_map(|text| {
            text_draw_geometry_in_rect(text, viewport_rect, root_scale)
                .map(|geometry| Item::Text(text, geometry))
        })
    }
}

/// The item `op` draws into `viewport_rect`, its records windowed by
/// `window`; texts are left out when `skip_text`. A text's and a run's
/// visibility tests work out their geometry and bounds, which their draws
/// take on.
fn op_item<'a>(
    scene: &'a CompositorScene,
    op: &DrawOp,
    window: Option<std::ops::Range<u32>>,
    (viewport_rect, root_scale, skip_text): (Rect, f32, bool),
) -> OpItem<'a> {
    let item = match op.kind {
        DrawOpKind::Text(index) => {
            let text = &scene.texts[index];
            text_draw_geometry_in_rect(text, viewport_rect, root_scale)
                .filter(|_| !skip_text)
                .map(|geometry| Item::Text(text, geometry))
        }
        DrawOpKind::Run(index) => {
            let run = &scene.runs[index];
            visible_run_bounds(run, viewport_rect, root_scale)
                .filter(|_| run_has_shapes(run))
                .map(|bounds| Item::Run(run, window, bounds))
        }
        _ if !op_is_visible_in_rect(scene, op, viewport_rect, root_scale) => None,
        DrawOpKind::Shadow(index) => {
            let shadow = &scene.shadow_draws[index];
            let casters = shadow.shapes.as_ref().and_then(|run| {
                visible_run_bounds(run, viewport_rect, root_scale)
                    .map(|bounds| Item::Run(run, None, bounds))
            });
            return OpItem::Shadow(shadow.texts.iter(), casters);
        }
        DrawOpKind::Image(index) => Some(Item::Image(index)),
        DrawOpKind::RRectShadow(index) => Some(Item::RRectShadow(&scene.rrect_shadows[index])),
    };
    item.map_or(OpItem::Nothing, OpItem::Draw)
}

/// The draw bounds of `run` (see [`run_draw_bounds`]) when it draws into
/// `viewport_rect`.
fn visible_run_bounds(run: &RunDraw, viewport_rect: Rect, root_scale: f32) -> Option<Rect> {
    run_draw_bounds(run, root_scale).filter(|bounds| bounds.intersect(viewport_rect).is_some())
}
