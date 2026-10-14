use std::rc::Rc;

use crate::{
    draw::{ClipBounds, RenderItem},
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
        z_index: _,
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

fn same_text(old: &TextDraw, new: &TextDraw) -> bool {
    let TextDraw {
        rect,
        snap_anchor,
        text,
        color,
        text_style,
        font_size,
        scale,
        z_index: _,
        clip,
    } = old;
    *rect == new.rect
        && *snap_anchor == new.snap_anchor
        && (Rc::ptr_eq(text, &new.text) || **text == *new.text)
        && *color == new.color
        && *text_style == new.text_style
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
        z_index: _,
        clip,
        blend_mode,
        src_rect,
    } = old;
    *rect == new.rect
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
        z_index: _,
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

pub(crate) fn changed_areas(
    old: &RasterScene,
    new: &RasterScene,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) {
    areas.clear();
    let old_items = old.ordered_items();
    let new_items = new.ordered_items();
    let common_start = old_items
        .iter()
        .zip(&new_items)
        .take_while(|(a, b)| same_item(old, **a, new, **b))
        .count();
    let common_end = old_items[common_start..]
        .iter()
        .rev()
        .zip(new_items[common_start..].iter().rev())
        .take_while(|(a, b)| same_item(old, **a, new, **b))
        .count();
    let old_changed = &old_items[common_start..old_items.len() - common_end];
    let new_changed = &new_items[common_start..new_items.len() - common_end];
    let changed = old_changed
        .iter()
        .filter_map(|item| old.item_bounds(*item, window))
        .chain(
            new_changed
                .iter()
                .filter_map(|item| new.item_bounds(*item, window)),
        );
    for area in changed {
        add_area(areas, area);
    }
    if areas.len() > MAX_AREAS {
        let around = areas.iter().copied().reduce(union);
        areas.clear();
        areas.extend(around);
    }
}
