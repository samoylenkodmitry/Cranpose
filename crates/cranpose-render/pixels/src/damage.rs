use std::{rc::Rc, sync::Arc};

use cranpose_ui_graphics::{Brush, CornerRadii, Rect};

use crate::{
    draw::{ClipBounds, RenderItem, ShapeCoverage, clip_rect_to_window, place_shape},
    scene::{DrawShape, ImageDraw, RasterScene, ShadowDraw, TextDraw},
};

const MAX_AREAS: usize = 8;

fn same_shape(old: &DrawShape, new: &DrawShape) -> bool {
    let DrawShape {
        rect,
        snap_anchor,
        snap_to_pixel_grid,
        brush,
        shape,
        stroke,
        arc,
        line,
        trapezoid,
        clip,
        blend_mode,
    } = old;
    *rect == new.rect
        && *snap_anchor == new.snap_anchor
        && *snap_to_pixel_grid == new.snap_to_pixel_grid
        && *brush == new.brush
        && *shape == new.shape
        && *stroke == new.stroke
        && *arc == new.arc
        && *line == new.line
        && *trapezoid == new.trapezoid
        && *clip == new.clip
        && *blend_mode == new.blend_mode
}

/// Whether `shape` fills its rect with one colour: no stroke, arc, line or
/// slice.
fn solid_fill(shape: &DrawShape) -> bool {
    matches!(shape.brush, Brush::Solid(_))
        && shape.stroke.is_none()
        && shape.arc.is_none()
        && shape.line.is_none()
        && shape.trapezoid.is_none()
}

/// The placed rects of `old` and `new` and their corners when both are one
/// solid fill that differs only in where it lies, with the same corners.
fn moved_fill(old: &DrawShape, new: &DrawShape) -> Option<(Rect, Rect, CornerRadii)> {
    let DrawShape {
        rect: _,
        snap_anchor: _,
        snap_to_pixel_grid,
        brush,
        shape,
        stroke: _,
        arc: _,
        line: _,
        trapezoid: _,
        clip,
        blend_mode,
    } = old;
    let alike = solid_fill(old)
        && solid_fill(new)
        && *snap_to_pixel_grid == new.snap_to_pixel_grid
        && *brush == new.brush
        && *shape == new.shape
        && *clip == new.clip
        && *blend_mode == new.blend_mode;
    if !alike {
        return None;
    }
    let (
        ShapeCoverage::Fill {
            rect: a,
            radii: old_radii,
        },
        ShapeCoverage::Fill {
            rect: b,
            radii: new_radii,
        },
    ) = (place_shape(old)?.coverage, place_shape(new)?.coverage)
    else {
        return None;
    };
    (old_radii == new_radii).then(|| (a, b, old_radii.unwrap_or_default()))
}

/// A strip along each edge that moved from `a` to `b`, from its old to its
/// new place and in by that side's corner radius, across both rects.
fn edge_strips(a: Rect, b: Rect, radii: CornerRadii) -> [Option<Rect>; 4] {
    let (a_right, b_right) = (a.x + a.width, b.x + b.width);
    let (a_bottom, b_bottom) = (a.y + a.height, b.y + b.height);
    let (left, right) = (a.x.min(b.x), a_right.max(b_right));
    let (top, bottom) = (a.y.min(b.y), a_bottom.max(b_bottom));
    let strip = |x: f32, to_x: f32, y: f32, to_y: f32| Rect {
        x,
        y,
        width: to_x - x,
        height: to_y - y,
    };
    let left_reach = radii.top_left.max(radii.bottom_left);
    let right_reach = radii.top_right.max(radii.bottom_right);
    let top_reach = radii.top_left.max(radii.top_right);
    let bottom_reach = radii.bottom_left.max(radii.bottom_right);
    [
        (a.x != b.x).then(|| strip(left, a.x.max(b.x) + left_reach, top, bottom)),
        (a_right != b_right).then(|| strip(a_right.min(b_right) - right_reach, right, top, bottom)),
        (a.y != b.y).then(|| strip(left, right, top, a.y.max(b.y) + top_reach)),
        (a_bottom != b_bottom)
            .then(|| strip(left, right, a_bottom.min(b_bottom) - bottom_reach, bottom)),
    ]
}

/// Adds the areas where `new` can draw other pixels than `old` when both are
/// one solid fill that differs only in where it lies, with the same corners:
/// the strips along its moved edges. Away from them each pixel lies inside
/// or outside both fills alike. Returns false and adds nothing for any other
/// pair.
fn add_moved_fill_areas(
    old: &DrawShape,
    new: &DrawShape,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) -> bool {
    let Some((a, b, radii)) = moved_fill(old, new) else {
        return false;
    };
    for strip in edge_strips(a, b, radii).into_iter().flatten() {
        if let Some(area) = clip_rect_to_window(strip, old.clip, window) {
            add_area(areas, area);
        }
    }
    true
}

fn same_text(old: &TextDraw, new: &TextDraw) -> bool {
    let TextDraw {
        rect,
        snap_anchor,
        text,
        color,
        text_style,
        font_size,
        scale,
        clip,
    } = old;
    *rect == new.rect
        && *snap_anchor == new.snap_anchor
        && (Rc::ptr_eq(text, &new.text) || **text == *new.text)
        && (Arc::ptr_eq(text_style, &new.text_style) || **text_style == *new.text_style)
        && *color == new.color
        && *font_size == new.font_size
        && *scale == new.scale
        && *clip == new.clip
}

fn same_image(old: &ImageDraw, new: &ImageDraw) -> bool {
    let ImageDraw {
        rect,
        snap_anchor,
        image,
        alpha,
        color_filter,
        sampling,
        clip,
        blend_mode,
        src_rect,
        transform,
    } = old;
    *rect == new.rect
        && *transform == new.transform
        && *snap_anchor == new.snap_anchor
        && *image == new.image
        && *alpha == new.alpha
        && *color_filter == new.color_filter
        && *sampling == new.sampling
        && *clip == new.clip
        && *blend_mode == new.blend_mode
        && *src_rect == new.src_rect
}

fn same_shadow(old: &ShadowDraw, new: &ShadowDraw) -> bool {
    let ShadowDraw {
        shadow,
        snap_anchor,
        color,
        clip,
    } = old;
    *shadow == new.shadow
        && *snap_anchor == new.snap_anchor
        && *color == new.color
        && *clip == new.clip
}

fn same_item(
    old: &RasterScene,
    old_item: RenderItem,
    new: &RasterScene,
    new_item: RenderItem,
) -> bool {
    match (old_item, new_item) {
        (RenderItem::Shape(a), RenderItem::Shape(b)) => same_shape(&old.shapes[a], &new.shapes[b]),
        (RenderItem::Text(a), RenderItem::Text(b)) => same_text(&old.texts[a], &new.texts[b]),
        (RenderItem::Image(a), RenderItem::Image(b)) => same_image(&old.images[a], &new.images[b]),
        (RenderItem::Shadow(a), RenderItem::Shadow(b)) => {
            same_shadow(&old.shadows[a], &new.shadows[b])
        }
        _ => false,
    }
}

fn union(a: ClipBounds, b: ClipBounds) -> ClipBounds {
    ClipBounds {
        min_x: a.min_x.min(b.min_x),
        min_y: a.min_y.min(b.min_y),
        max_x: a.max_x.max(b.max_x),
        max_y: a.max_y.max(b.max_y),
    }
}

fn touch(a: ClipBounds, b: ClipBounds) -> bool {
    a.min_x <= b.max_x && b.min_x <= a.max_x && a.min_y <= b.max_y && b.min_y <= a.max_y
}

fn add_area(areas: &mut Vec<ClipBounds>, mut area: ClipBounds) {
    while let Some(index) = areas.iter().position(|other| touch(*other, area)) {
        area = union(areas.swap_remove(index), area);
    }
    areas.push(area);
}

/// How many items `old` and `new` share at their start and at their end.
/// The runs `new` copied in place from `old` are equal without a comparison.
fn common_ends(old: &RasterScene, new: &RasterScene) -> (usize, usize) {
    let (old_items, new_items) = (&old.order, &new.order);
    let shared = old_items.len().min(new_items.len());
    let mut start = 0;
    let mut runs = new.kept.iter().peekable();
    while start < shared {
        if let Some(run) = runs.next_if(|run| run.start <= start) {
            start = start.max(run.end.min(shared));
            continue;
        }
        if !same_item(old, old_items[start], new, new_items[start]) {
            break;
        }
        start += 1;
    }
    let rest = shared - start;
    // A run copied in place after the change keeps its index only where
    // both scenes hold as many items.
    let same_length = old_items.len() == new_items.len();
    let mut end = 0;
    let mut runs = new.kept.iter().rev().peekable();
    while end < rest {
        let index = new_items.len() - 1 - end;
        if same_length && let Some(run) = runs.next_if(|run| run.end > index) {
            if run.start <= index {
                end = (new_items.len() - run.start).min(rest);
            }
            continue;
        }
        if !same_item(
            old,
            old_items[old_items.len() - 1 - end],
            new,
            new_items[index],
        ) {
            break;
        }
        end += 1;
    }
    (start, end)
}

pub(crate) fn changed_areas(
    old: &RasterScene,
    new: &RasterScene,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) {
    areas.clear();
    let old_items = &old.order;
    let new_items = &new.order;
    let (common_start, common_end) = common_ends(old, new);
    let old_changed = &old_items[common_start..old_items.len() - common_end];
    let new_changed = &new_items[common_start..new_items.len() - common_end];
    let add_bounds = |areas: &mut Vec<ClipBounds>, scene: &RasterScene, item: RenderItem| {
        if let Some(area) = scene.item_bounds(item, window) {
            add_area(areas, area);
        }
    };
    if old_changed.len() != new_changed.len() {
        old_changed
            .iter()
            .for_each(|item| add_bounds(areas, old, *item));
        new_changed
            .iter()
            .for_each(|item| add_bounds(areas, new, *item));
    } else {
        // Lists of one length change item by item: a fill that only moved
        // changes the strips along its moved edges.
        for (old_item, new_item) in old_changed.iter().zip(new_changed) {
            let moved = match (*old_item, *new_item) {
                (RenderItem::Shape(a), RenderItem::Shape(b)) => {
                    add_moved_fill_areas(&old.shapes[a], &new.shapes[b], window, areas)
                }
                _ => false,
            };
            if !moved {
                add_bounds(areas, old, *old_item);
                add_bounds(areas, new, *new_item);
            }
        }
    }
    if areas.len() > MAX_AREAS {
        let around = areas.iter().copied().reduce(union);
        areas.clear();
        areas.extend(around);
    }
}
