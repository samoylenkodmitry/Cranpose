// What a later opaque fill hides of the fills laid before it in one arena
// chunk, cut where the chunk's records are laid. A fill whole under the
// interior of a later opaque fill draws nothing. A solid fill partly under
// one draws as the strip of four quads around the hidden part. The vertex
// stage builds the strip from the hole that the record keeps in
// `arc_geometry`. The records between the two draw into the same render
// pass, one after the other. None of them reads the target, so the opaque
// interior covers the hidden pixels whatever was drawn there before it. The
// search keeps to the draws since the chunk's last cut. A cut draws held
// glyphs, images, a stored run or a composite between the chunk's draws,
// then clears the kept fills. Every pipeline that a solid draw can take by
// a join shades an unturned record from the fragment position and flat
// values: the pipeline of vertex gradients gives a record without one its
// colour whole.

use cranpose_ui_graphics::{
    BlendMode, FRAGMENT_KIND_FILL, OCCLUDER_MIN_AREA, ShapeRecordBody, ShapeRecordCurve,
    band_class_segments, interior_occludes, record_fragment_kind, strip_vertices,
};

use super::{PlacementData, RunDrawCall};
use crate::{
    ablation::Ablation,
    render::ShapePipelineKey,
    run_geometry::ShapeFill,
    scene::{PLACEMENT_CLIPPED, PLACEMENT_FILTERED, PLACEMENT_PAINTED, PLACEMENT_TURNED},
};

// The record flag of a framed fill, above every flag `cranpose_ui_graphics`
// packs (`RECORD_FRAMED` in shape.wgsl).
const RECORD_FRAMED: u32 = 1 << 25;
// The band class of a framed fill's draw: a strip of four quads. A quad of
// the same draw leaves its later vertices on its last corner, so it draws
// as before.
const FRAME_CLASS: u8 = 2;
// Whether a draw at band class `class` may join the draw at `into` before
// it: at the same class, or as quads joining a strip of the frame class.
// Each quad leaves its later vertices on its last corner. Without this join
// every framed fill would split the draws of the records after it.
pub(super) fn joins(into: u8, class: u8) -> bool {
    into == class || (into == FRAME_CLASS && class == 0)
}

// The vertices `draw` adds past its own by joining a draw at class `into`:
// its records instance over the longer strip.
pub(super) fn joined_vertices(into: Option<u8>, draw: &RunDrawCall) -> u64 {
    into.filter(|&into| into != draw.band_class)
        .map_or(0, |into| {
            widened_vertices(draw.band_class, into, &draw.records)
        })
}

// The vertices `records`, drawn at band class `from`, add when they instance
// over the strip of class `to` instead.
fn widened_vertices(from: u8, to: u8, records: &std::ops::Range<u32>) -> u64 {
    let added = strip_vertices(band_class_segments(to)) - strip_vertices(band_class_segments(from));
    u64::from(added) * u64::from(records.end - records.start)
}

// The count of latest fills kept for a later opaque interior to hide. An
// opaque fill hides what was painted shortly before it: a nested background
// hides its parent's background, a card hides the panel under it.
const KEPT_FILLS: usize = 8;

// A rect in the device pixels where the vertex stage places a chunk's
// records: left, top, right, bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DeviceRect([f32; 4]);

impl DeviceRect {
    // The rect that `body` rasterizes under `placement`, as
    // `record_geometry` places an unstroked record.
    fn of(body: &ShapeRecordBody, placement: &PlacementData) -> Self {
        let [x, y, width, height] = body.rect;
        let [dx, dy] = placement.offset;
        let scale = placement.root_scale;
        Self([
            (x + dx) * scale,
            (y + dy) * scale,
            (x + width + dx) * scale,
            (y + height + dy) * scale,
        ])
    }

    fn area(self) -> f32 {
        let [left, top, right, bottom] = self.0;
        (right - left).max(0.0) * (bottom - top).max(0.0)
    }

    fn contains(self, other: Self) -> bool {
        let [left, top, right, bottom] = self.0;
        let [other_left, other_top, other_right, other_bottom] = other.0;
        left <= other_left && top <= other_top && right >= other_right && bottom >= other_bottom
    }

    fn intersection(self, other: Self) -> Self {
        let [left, top, right, bottom] = self.0;
        let [other_left, other_top, other_right, other_bottom] = other.0;
        Self([
            left.max(other_left),
            top.max(other_top),
            right.min(other_right),
            bottom.min(other_bottom),
        ])
    }
}

// A fill laid since the chunk's last cut.
#[derive(Clone, Copy, Debug)]
struct KeptFill {
    // Its index in the chunk's records. Its draw's index in the chunk's draws.
    record: usize,
    draw: usize,
    rect: DeviceRect,
    // Whether its pixels take their colour from the fragment position and
    // flat values alone, so a strip shades each one as the quad did.
    shades_flat: bool,
    // The device area of its part that later interiors hide so far. The
    // least area worth a frame: the strip costs its draw six more vertices
    // a record.
    hidden: f32,
    least_hidden: f32,
}

#[derive(Debug, Default)]
pub(super) struct OcclusionCut {
    kept: Vec<KeptFill>,
    // `CRANPOSE_ABLATE=occlusion_cut`, read as each chunk opens: every fill
    // draws whole.
    left_out: bool,
}

impl OcclusionCut {
    pub(super) fn open(&mut self) {
        self.kept.clear();
        self.left_out = Ablation::current().occlusion_cut;
    }

    pub(super) fn clear(&mut self) {
        self.kept.clear();
    }

    // Cuts what each of `records`, just laid under `key`, hides of the fills
    // laid before it, then keeps it for the records after it. `draw_index`
    // is the chunk's draw that holds them.
    pub(super) fn lay(
        &mut self,
        (bodies, curves, placements): (
            &mut [ShapeRecordBody],
            &[ShapeRecordCurve],
            &[PlacementData],
        ),
        (draws, draw_index): (&mut [RunDrawCall], usize),
        (key, records): (ShapePipelineKey, std::ops::Range<u32>),
        fill: &mut ShapeFill,
    ) {
        if self.left_out {
            return;
        }
        let shades_flat = key.variant.shades_flat();
        let src_over = key.blend_mode == BlendMode::SrcOver;
        for index in records.start as usize..records.end as usize {
            let body = &bodies[index];
            if record_fragment_kind(body.flags) != FRAGMENT_KIND_FILL {
                continue;
            }
            let placement = &placements[body.placement as usize];
            if placement.flags & PLACEMENT_TURNED != 0 {
                continue;
            }
            let rect = DeviceRect::of(body, placement);
            let interior = (src_over && interior_occludes(body, &curves[index]))
                .then(|| opaque_interior(rect, &curves[index], placement))
                .flatten();
            let [_, _, width, height] = body.rect;
            let kept = (width * height >= OCCLUDER_MIN_AREA).then_some(KeptFill {
                record: index,
                draw: draw_index,
                rect,
                shades_flat: shades_flat && body.brush == 0,
                hidden: 0.0,
                least_hidden: OCCLUDER_MIN_AREA * placement.root_scale * placement.root_scale,
            });
            if let Some(interior) = interior {
                self.hide(interior, (bodies, draws), fill);
            }
            if let Some(kept) = kept {
                self.keep(kept);
            }
        }
    }

    // Keeps `fill` in the place of the earliest laid kept fill once the list
    // is full, so no kept fill moves.
    fn keep(&mut self, fill: KeptFill) {
        if self.kept.len() < KEPT_FILLS {
            self.kept.push(fill);
        } else if let Some(earliest) = self.kept.iter_mut().min_by_key(|kept| kept.record) {
            *earliest = fill;
        }
    }

    // Drops the kept fills that `interior` covers whole. Frames each kept
    // fill whose hidden part `interior` makes larger than before.
    fn hide(
        &mut self,
        interior: DeviceRect,
        (bodies, draws): (&mut [ShapeRecordBody], &mut [RunDrawCall]),
        fill: &mut ShapeFill,
    ) {
        self.kept.retain_mut(|kept| {
            let body = &mut bodies[kept.record];
            if interior.contains(kept.rect) {
                fill.hide(body, f64::from(kept.rect.area() - kept.hidden));
                body.rect[2] = 0.0;
                body.rect[3] = 0.0;
                body.flags &= !RECORD_FRAMED;
                return false;
            }
            let hole = interior.intersection(kept.rect);
            let hidden = hole.area();
            if kept.shades_flat && hidden >= kept.least_hidden && hidden > kept.hidden {
                fill.hide(body, f64::from(hidden - kept.hidden));
                body.flags |= RECORD_FRAMED;
                body.arc_geometry = hole.0;
                let draw = &mut draws[kept.draw];
                if draw.band_class < FRAME_CLASS {
                    fill.vertices += widened_vertices(draw.band_class, FRAME_CLASS, &draw.records);
                    draw.band_class = FRAME_CLASS;
                }
                kept.hidden = hidden;
            }
            true
        });
    }
}

// The part of an opaque fill at `rect` where the fill covers every pixel
// centre with its whole colour: the rect inset by its largest corner radius
// and a pixel, cut to its clip, with its edges on whole pixels. `None` under
// a paint that lets anything through (an alpha below 1, a colour filter),
// under a rounded clip, or when nothing is left.
fn opaque_interior(
    rect: DeviceRect,
    curve: &ShapeRecordCurve,
    placement: &PlacementData,
) -> Option<DeviceRect> {
    let paint_passes = placement.flags & PLACEMENT_PAINTED == 0 || placement.alpha >= 1.0;
    if !paint_passes || placement.flags & PLACEMENT_FILTERED != 0 || placement.clip_radius > 0.0 {
        return None;
    }
    let radius = curve
        .radii
        .iter()
        .fold(0.0_f32, |largest, &radius| largest.max(radius));
    let inset = radius * placement.root_scale + 1.0;
    let [left, top, right, bottom] = rect.0;
    let mut interior = DeviceRect([left + inset, top + inset, right - inset, bottom - inset]);
    if placement.flags & PLACEMENT_CLIPPED != 0 {
        let [x, y, width, height] = placement.clip;
        interior = interior.intersection(DeviceRect([x, y, x + width, y + height]));
    }
    let [left, top, right, bottom] = interior.0;
    let snapped = DeviceRect([left.ceil(), top.ceil(), right.floor(), bottom.floor()]);
    let [left, top, right, bottom] = snapped.0;
    (right > left && bottom > top).then_some(snapped)
}
