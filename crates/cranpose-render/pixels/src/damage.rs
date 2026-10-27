use std::{rc::Rc, sync::Arc};

use cranpose_ui_graphics::{ArcGeometry, Brush, CornerRadii, Rect, TAU};

use crate::{
    draw::{ClipBounds, RenderItem, ShapeCoverage, clip_rect_to_window, place_shape},
    scene::{DrawShape, ImageDraw, Outline, RasterScene, ShadowDraw, TextDraw},
};

const MAX_AREAS: usize = 8;

fn same_shape(old: &DrawShape, new: &DrawShape) -> bool {
    let DrawShape {
        rect,
        snap_anchor,
        snap_to_pixel_grid,
        brush,
        outline,
        clip,
        blend_mode,
    } = old;
    *rect == new.rect
        && *snap_anchor == new.snap_anchor
        && *snap_to_pixel_grid == new.snap_to_pixel_grid
        && *brush == new.brush
        && *outline == new.outline
        && *clip == new.clip
        && *blend_mode == new.blend_mode
}

/// Whether `shape` fills its rect with one colour: no stroke, arc, line or
/// slice.
fn solid_fill(shape: &DrawShape) -> bool {
    matches!(shape.brush, Brush::Solid(_)) && matches!(shape.outline, Outline::Fill(_))
}

/// The placed rects of `old` and `new` and their corners when both are one
/// solid fill that differs only in where it lies, with the same corners.
fn moved_fill(old: &DrawShape, new: &DrawShape) -> Option<(Rect, Rect, CornerRadii)> {
    let DrawShape {
        rect: _,
        snap_anchor: _,
        snap_to_pixel_grid,
        brush,
        outline,
        clip,
        blend_mode,
    } = old;
    let alike = solid_fill(old)
        && solid_fill(new)
        && *snap_to_pixel_grid == new.snap_to_pixel_grid
        && *brush == new.brush
        && *outline == new.outline
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

// The sweeps of `old` and `new`, the shorter first, when both are one arc
// that differs only in its sweep.
fn swept_sweeps(old: &DrawShape, new: &DrawShape) -> Option<(f32, f32)> {
    let DrawShape {
        rect,
        snap_anchor,
        snap_to_pixel_grid,
        brush,
        outline,
        clip,
        blend_mode,
    } = old;
    let (Outline::Arc(a), Outline::Arc(b)) = (*outline, new.outline) else {
        return None;
    };
    // A whole turn has no end of its own: it starts at angle zero.
    let partial = |arc: ArcGeometry| !arc.is_degenerate() && arc.sweep_angle < TAU;
    // An arc's rect is its bounds, which move with its sweep, and only a
    // gradient reads it.
    let alike = (*rect == new.rect || matches!(brush, Brush::Solid(_)))
        && *snap_anchor == new.snap_anchor
        && *snap_to_pixel_grid == new.snap_to_pixel_grid
        && *brush == new.brush
        && *clip == new.clip
        && *blend_mode == new.blend_mode
        && partial(a)
        && partial(b)
        && ArcGeometry {
            sweep_angle: b.sweep_angle,
            ..a
        } == b;
    alike.then_some(if a.sweep_angle < b.sweep_angle {
        (a.sweep_angle, b.sweep_angle)
    } else {
        (b.sweep_angle, a.sweep_angle)
    })
}

// Adds the area where `new` can draw other pixels than `old` when both are
// one arc that differs only in its sweep: around the part between their
// ends, caps included. Away from it a point lies inside or outside both
// sweeps alike, and its coverage reads only the end it lies near. Returns
// false and adds nothing for any other pair.
fn add_swept_arc_areas(
    old: &DrawShape,
    new: &DrawShape,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) -> bool {
    let Some((shorter, longer)) = swept_sweeps(old, new) else {
        return false;
    };
    if let Some(placed) = place_shape(old)
        && let ShapeCoverage::Arc(arc) = placed.coverage
    {
        let between = ShapeCoverage::Arc(ArcGeometry {
            start_angle: arc.start_angle + shorter,
            sweep_angle: longer - shorter,
            ..arc
        });
        if let Some(area) = clip_rect_to_window(between.reach(placed.rect), old.clip, window) {
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
    // No two areas the list holds touch, so an area inside the last one
    // touches no other: the loop would take the last one out and put it
    // back as it was.
    if areas.last().is_some_and(|last| last.holds(area)) {
        return;
    }
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
    old_reach: &[Option<ClipBounds>],
    new: &RasterScene,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) {
    areas.clear();
    let old_items = &old.order;
    let new_items = &new.order;
    let (common_start, common_end) = common_ends(old, new);
    let changed = common_start..old_items.len() - common_end;
    let reach = match old_reach.get(changed.clone()) {
        Some(reach) if old_reach.len() == old_items.len() => RunReach::Kept(reach),
        _ => RunReach::Unknown,
    };
    add_changed_runs(
        (old, &old_items[changed]),
        (new, &new_items[common_start..new_items.len() - common_end]),
        reach,
        window,
        areas,
    );
    merge_areas(areas);
}

// Where the items of an old run reach in the window, as the scene's reach
// list holds them.
pub(crate) enum RunReach<'a> {
    // Not held: each old item's reach is worked out.
    Unknown,
    // Held, and kept as it is.
    Kept(&'a [Option<ClipBounds>]),
    // Held, and each new item's reach takes its old item's place: the new
    // run takes the old run's place in the order, item for item.
    Replaced(&'a mut [Option<ClipBounds>]),
}

impl RunReach<'_> {
    fn old(
        &self,
        scene: &RasterScene,
        index: usize,
        item: RenderItem,
        window: ClipBounds,
    ) -> Option<ClipBounds> {
        match self {
            Self::Unknown => scene.item_bounds(item, window),
            Self::Kept(reach) => reach[index],
            Self::Replaced(reach) => reach[index],
        }
    }

    fn replaces(&self) -> bool {
        matches!(self, Self::Replaced(_))
    }

    fn replace(&mut self, index: usize, reach: Option<ClipBounds>) {
        if let Self::Replaced(held) = self {
            held[index] = reach;
        }
    }
}

// Adds to `areas` where the run `old.1` of the scene `old.0` and the run
// `new.1` of `new.0`, which take each other's place in one draw order, draw
// differently. Items paired in order draw in the same order in both scenes,
// so a pixel no changed pair and no unpaired item reaches keeps its colour.
// An item drawn as its pair was reaches where its pair did.
pub(crate) fn add_changed_runs(
    (old, old_items): (&RasterScene, &[RenderItem]),
    (new, new_items): (&RasterScene, &[RenderItem]),
    mut reach: RunReach<'_>,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) {
    let paired = old_items.len().min(new_items.len());
    for (index, (&old_item, &new_item)) in old_items.iter().zip(new_items).enumerate() {
        if same_item(old, old_item, new, new_item) {
            continue;
        }
        // A moved fill and a swept arc add only the areas their change
        // reaches.
        let narrowed = match (old_item, new_item) {
            (RenderItem::Shape(a), RenderItem::Shape(b)) => {
                let (old_shape, new_shape) = (&old.shapes[a], &new.shapes[b]);
                add_moved_fill_areas(old_shape, new_shape, window, areas)
                    || add_swept_arc_areas(old_shape, new_shape, window, areas)
            }
            _ => false,
        };
        if narrowed && !reach.replaces() {
            continue;
        }
        if !narrowed && let Some(area) = reach.old(old, index, old_item, window) {
            add_area(areas, area);
        }
        let new_bounds = new.item_bounds(new_item, window);
        if !narrowed && let Some(area) = new_bounds {
            add_area(areas, area);
        }
        reach.replace(index, new_bounds);
    }
    for (index, &item) in old_items.iter().enumerate().skip(paired) {
        if let Some(area) = reach.old(old, index, item, window) {
            add_area(areas, area);
        }
    }
    for &item in &new_items[paired..] {
        if let Some(area) = new.item_bounds(item, window) {
            add_area(areas, area);
        }
    }
}

fn pixels(area: ClipBounds) -> i64 {
    i64::from(area.max_x - area.min_x) * i64::from(area.max_y - area.min_y)
}

// Joins `areas` while they are too many to draw one by one, each time the
// two whose join takes in the fewest pixels that neither covers, so that
// changes far apart do not draw the still pixels between them again.
pub(crate) fn merge_areas(areas: &mut Vec<ClipBounds>) {
    while areas.len() > MAX_AREAS {
        let mut cheapest = (0, 1, i64::MAX);
        for (first, &a) in areas.iter().enumerate() {
            for (second, &b) in areas.iter().enumerate().skip(first + 1) {
                let extra = pixels(union(a, b)) - pixels(a) - pixels(b);
                if extra < cheapest.2 {
                    cheapest = (first, second, extra);
                }
            }
        }
        let (first, second, _) = cheapest;
        let joined = union(areas.swap_remove(second), areas.swap_remove(first));
        add_area(areas, joined);
    }
}
