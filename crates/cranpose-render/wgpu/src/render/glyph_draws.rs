use std::rc::Rc;

use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::Rect;

use super::{
    CachedGpuTextGlyphRun, GlyphRunQuads, TargetRect, ViewportUniformParams,
    cached_text_glyph_instance, cached_text_glyph_quad_is_visible_in_viewport, confined_scissor,
    target_rect_within,
};
use crate::{
    frame_graph::UploadAllocatorSpec,
    geometry::{SegmentTransform, canonicalize_device_coordinate},
    idle_pool::FrameScratch,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphExtent {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl GlyphExtent {
    pub(crate) const EMPTY: Self = Self {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };

    pub(crate) fn add(&mut self, rect: [f32; 4]) {
        self.left = self.left.min(rect[0]);
        self.top = self.top.min(rect[1]);
        self.right = self.right.max(rect[2]);
        self.bottom = self.bottom.max(rect[3]);
    }
}

/// The scissor an unturned shared glyph run needs and the target pixels it
/// touches. A run whose quads all lie inside `scissor` needs none of its own,
/// which lets it share a draw with its neighbours. A turned run never needs
/// one: its quads were cut to its clip before the turn.
pub(super) fn shared_glyph_clip(
    extent: GlyphExtent,
    scissor: TargetRect,
    viewport: ViewportUniformParams,
) -> (Option<TargetRect>, TargetRect) {
    let GlyphExtent {
        left,
        top,
        right,
        bottom,
    } = extent;
    if left > right {
        return (Some(scissor), scissor);
    }
    let left = (left - viewport.offset[0]).floor().max(0.0);
    let top = (top - viewport.offset[1]).floor().max(0.0);
    let right = (right - viewport.offset[0]).ceil();
    let bottom = (bottom - viewport.offset[1]).ceil();
    let (x, y, width, height) = scissor;
    let inside = left >= x as f32
        && top >= y as f32
        && right <= (x + width) as f32
        && bottom <= (y + height) as f32;
    if !inside || right <= left || bottom <= top {
        return (Some(scissor), scissor);
    }
    let bounds = (
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    );
    (None, bounds)
}

/// The corners a glyph instance draws as a triangle strip: top-left,
/// top-right, bottom-left, bottom-right.
pub(super) const GLYPH_QUAD_CORNERS: u32 = 4;

/// One glyph quad as the glyph pipeline draws it: an instance whose four
/// corners the vertex stage picks from `rect` and `uv`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct GlyphInstance {
    pub(super) rect: [f32; 4],
    pub(super) uv: [f32; 4],
    pub(super) uv_bounds: [f32; 4],
    pub(super) color: [f32; 4],
}

impl GlyphInstance {
    const ATTRIBS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4
    ];

    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBS,
        }
    }

    /// The part of the quad inside `edges` (left, top, right, bottom, in the
    /// quad's own space), its atlas coordinates cut to match; `None` when
    /// none of it is inside.
    pub(crate) fn clipped_to(self, edges: [f32; 4]) -> Option<Self> {
        let [x0, y0, x1, y1] = self.rect;
        let rect = [
            x0.max(edges[0]),
            y0.max(edges[1]),
            x1.min(edges[2]),
            y1.min(edges[3]),
        ];
        if rect[2] <= rect[0] || rect[3] <= rect[1] {
            return None;
        }
        if rect == self.rect {
            return Some(self);
        }
        let [u0, v0, u1, v1] = self.uv;
        let u = |x: f32| u0 + (x - x0) / (x1 - x0) * (u1 - u0);
        let v = |y: f32| v0 + (y - y0) / (y1 - y0) * (v1 - v0);
        Some(Self {
            rect,
            uv: [u(rect[0]), v(rect[1]), u(rect[2]), v(rect[3])],
            ..self
        })
    }
}

/// Hands `sink` each quad of a glyph run at `source_raster_rect` that the
/// viewport shows, as an instance cut to `cut` when the text is cut.
#[inline]
pub(super) fn for_each_visible_glyph(
    source_raster_rect: Rect,
    quads: GlyphRunQuads<'_>,
    (clip, cut): (Option<Rect>, Option<[f32; 4]>),
    viewport: ViewportUniformParams,
    root_scale: f32,
    mut sink: impl FnMut(GlyphInstance),
) {
    let whole_run_visible =
        quads
            .bounds
            .within_viewport(source_raster_rect, clip, viewport, root_scale);
    for quad in quads.iter() {
        if !whole_run_visible
            && !cached_text_glyph_quad_is_visible_in_viewport(
                source_raster_rect,
                &quad,
                clip,
                viewport,
                root_scale,
            )
        {
            continue;
        }
        let Some(glyph) = cached_text_glyph_instance(source_raster_rect, &quad) else {
            continue;
        };
        let glyph = match cut {
            Some(edges) => glyph.clipped_to(edges),
            None => Some(glyph),
        };
        if let Some(glyph) = glyph {
            sink(glyph);
        }
    }
}

/// A glyph quad of a layer drawn in place under a turn, with the turn that
/// places it: glyphs of layers turned differently then share a draw, and
/// every other glyph keeps [`GlyphInstance`]'s smaller layout.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct TurnedGlyph {
    glyph: GlyphInstance,
    /// The turn, row-major.
    turn: [f32; 4],
    /// The turn's translation in `xy`.
    translation: [f32; 4],
}

impl TurnedGlyph {
    const ATTRIBS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4
    ];

    pub(crate) fn new(glyph: GlyphInstance, turn: SegmentTransform) -> Self {
        let (linear, [x, y], _) = turn.uniform_parts();
        Self {
            glyph,
            turn: linear,
            translation: [x, y, 0.0, 0.0],
        }
    }

    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// The glyph quads a pass draws from its shared buffers: the plain ones and
/// those of layers drawn in place under a turn.
#[derive(Default)]
pub(crate) struct GlyphInstances {
    pub(crate) plain: FrameScratch<GlyphInstance>,
    pub(crate) turned: FrameScratch<TurnedGlyph>,
}

impl GlyphInstances {
    // How many glyphs the pass holds for a viewport turned by `turn`.
    pub(super) fn len_for(&self, turn: SegmentTransform) -> usize {
        if turn.is_identity() {
            self.plain.len()
        } else {
            self.turned.len()
        }
    }

    pub(crate) fn clear(&mut self) {
        self.plain.clear();
        self.turned.clear();
    }

    /// Ends a frame: see [`FrameScratch::end_frame`]. Passes need the
    /// most glyphs at startup, when nothing is cached yet.
    pub(crate) fn end_frame(&mut self) {
        self.plain.end_frame();
        self.turned.end_frame();
    }

    pub(super) fn lens(&self) -> (usize, usize) {
        (self.plain.len(), self.turned.len())
    }

    /// Moves the upright quads from `from` on by `shift` (see
    /// [`PlacementData::of`]).
    pub(crate) fn shift_plain(&mut self, from: usize, [x, y]: [f32; 2]) {
        for glyph in &mut self.plain[from..] {
            glyph.rect[0] += x;
            glyph.rect[1] += y;
            glyph.rect[2] += x;
            glyph.rect[3] += y;
        }
    }

    pub(super) fn truncate(&mut self, (plain, turned): (usize, usize)) {
        self.plain.truncate(plain);
        self.turned.truncate(turned);
    }
}

/// A text clip's edges in device pixels of the space its glyph quads are
/// laid out in, before a viewport transform moves them.
pub(super) fn glyph_clip_edges(clip: Rect, root_scale: f32) -> [f32; 4] {
    [
        canonicalize_device_coordinate(clip.x * root_scale),
        canonicalize_device_coordinate(clip.y * root_scale),
        canonicalize_device_coordinate((clip.x + clip.width) * root_scale),
        canonicalize_device_coordinate((clip.y + clip.height) * root_scale),
    ]
}

/// Where the glyph quads of a text are cut, in the quads' own space. Under a
/// turned viewport the cut is the text's `clip` itself, before the turn,
/// which no scissor can follow; the text then draws without one. Otherwise
/// it is the pixel edges of the text's `scissor`, its draw rect within its
/// clip: cut there, the quads cover exactly the pixels the scissor would
/// pass, with the same texels, so the text needs no scissor of its own and
/// draws in one call with its neighbours, even where a last line's
/// descenders or a glyph's padding reach past its draw rect.
pub(super) fn glyph_cut_edges(
    clip: Option<Rect>,
    scissor: TargetRect,
    viewport: ViewportUniformParams,
    root_scale: f32,
) -> Option<[f32; 4]> {
    if !viewport.transform.is_identity() {
        return clip.map(|clip| glyph_clip_edges(clip, root_scale));
    }
    let (x, y, width, height) = scissor;
    let [offset_x, offset_y] = viewport.offset;
    Some([
        x as f32 + offset_x,
        y as f32 + offset_y,
        (x + width) as f32 + offset_x,
        (y + height) as f32 + offset_y,
    ])
}

/// Which glyph pipeline draws a stretch of quads: plain quads, sampled
/// within their glyph's texel centers; plain quads on whole pixels at one
/// texel a pixel, whose samples land on those centers unclamped; or quads
/// of a layer drawn in place under a turn, which are [`TurnedGlyph`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlyphQuads {
    Plain,
    Aligned,
    Turned,
}

impl GlyphQuads {
    // The plain kind that fits a quad at `rect` in a target whose origin
    // sits at `offset`.
    pub(super) fn plain_at(rect: [f32; 4], offset: [f32; 2]) -> Self {
        if glyph_rect_aligned(rect, offset) {
            Self::Aligned
        } else {
            Self::Plain
        }
    }

    pub(super) fn turned(self) -> bool {
        self == Self::Turned
    }
}

/// Whether a plain glyph quad at `rect` lands on whole pixels of a target
/// whose origin sits at `offset`: its quads hold one texel a pixel.
pub(super) fn glyph_rect_aligned(rect: [f32; 4], offset: [f32; 2]) -> bool {
    let whole = |value: f32| value.fract() == 0.0;
    whole(rect[0] - offset[0])
        && whole(rect[1] - offset[1])
        && whole(rect[2] - offset[0])
        && whole(rect[3] - offset[1])
}

#[derive(Clone)]
enum GlyphDrawSource {
    Shared {
        instance_start: u32,
        instance_count: u32,
        quads: GlyphQuads,
    },
    Retained {
        run: Rc<CachedGpuTextGlyphRun>,
        uniform_slot: usize,
        aligned: bool,
    },
}

#[derive(Clone)]
pub(crate) struct GlyphDrawCmd {
    atlas: Rc<wgpu::BindGroup>,
    source: GlyphDrawSource,
    /// The scissor the text's own clip needs, `None` when its quads lie
    /// inside that clip anyway and only the batch's bound applies.
    scissor: Option<(u32, u32, u32, u32)>,
    /// Target pixels the draw can touch, for ordering it against others.
    bounds: (u32, u32, u32, u32),
}

/// One draw of a glyph batch: a stretch of shared quads, or a retained run.
pub(super) struct GlyphDraw<'a> {
    pub(super) atlas: &'a Rc<wgpu::BindGroup>,
    pub(super) scissor: Option<(u32, u32, u32, u32)>,
    pub(super) step: GlyphDrawStep<'a>,
}

pub(super) enum GlyphDrawStep<'a> {
    /// A stretch of the shared instances of one kind.
    Shared(std::ops::Range<u32>, GlyphQuads),
    Retained {
        run: &'a CachedGpuTextGlyphRun,
        uniform_slot: usize,
        aligned: bool,
    },
}

/// The draws a batch's glyph commands take: consecutive shared commands with
/// one atlas and one scissor whose quads follow each other in the frame's
/// shared buffer draw as one.
pub(super) struct GlyphDraws<'a> {
    cmds: &'a [GlyphDrawCmd],
}

impl<'a> GlyphDraws<'a> {
    pub(crate) fn new(cmds: &'a [GlyphDrawCmd]) -> Self {
        Self { cmds }
    }
}

impl<'a> Iterator for GlyphDraws<'a> {
    type Item = GlyphDraw<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (first, rest) = self.cmds.split_first()?;
        self.cmds = rest;
        let step = match &first.source {
            GlyphDrawSource::Retained {
                run,
                uniform_slot,
                aligned,
            } => GlyphDrawStep::Retained {
                run,
                uniform_slot: *uniform_slot,
                aligned: *aligned,
            },
            GlyphDrawSource::Shared {
                instance_start,
                instance_count,
                quads,
            } => {
                let mut end = instance_start + instance_count;
                while let Some((next, rest)) = self.cmds.split_first() {
                    match next.source {
                        GlyphDrawSource::Shared {
                            instance_start,
                            instance_count,
                            quads: next_quads,
                        } if instance_start == end
                            && next_quads == *quads
                            && next.scissor == first.scissor
                            && Rc::ptr_eq(&next.atlas, &first.atlas) =>
                        {
                            end += instance_count;
                            self.cmds = rest;
                        }
                        _ => break,
                    }
                }
                GlyphDrawStep::Shared(*instance_start..end, *quads)
            }
        };
        Some(GlyphDraw {
            atlas: &first.atlas,
            scissor: first.scissor,
            step,
        })
    }
}

impl GlyphDrawCmd {
    /// Keeps the draw within `slot` when its quads may reach past it.
    pub(crate) fn confine(&mut self, slot: TargetRect) {
        if !target_rect_within(self.bounds, slot) {
            self.scissor = Some(confined_scissor(self.scissor, slot));
        }
    }

    /// The quads the command draws, which pick its pipeline.
    pub(super) fn quads(&self) -> GlyphQuads {
        match &self.source {
            GlyphDrawSource::Shared { quads, .. } => *quads,
            GlyphDrawSource::Retained { aligned: true, .. } => GlyphQuads::Aligned,
            GlyphDrawSource::Retained { aligned: false, .. } => GlyphQuads::Plain,
        }
    }

    pub(crate) fn shared(
        (instances, quads): (std::ops::Range<usize>, GlyphQuads),
        scissor: Option<(u32, u32, u32, u32)>,
        bounds: (u32, u32, u32, u32),
        atlas: Rc<wgpu::BindGroup>,
    ) -> Self {
        let start = u32::try_from(instances.start).unwrap_or(u32::MAX);
        let end = u32::try_from(instances.end).unwrap_or(u32::MAX);
        Self {
            atlas,
            source: GlyphDrawSource::Shared {
                instance_start: start,
                instance_count: end.saturating_sub(start),
                quads,
            },
            scissor,
            bounds,
        }
    }

    pub(super) fn retained(
        (run, uniform_slot, aligned): (Rc<CachedGpuTextGlyphRun>, usize, bool),
        scissor: (u32, u32, u32, u32),
        atlas: Rc<wgpu::BindGroup>,
    ) -> Self {
        Self {
            atlas,
            source: GlyphDrawSource::Retained {
                run,
                uniform_slot,
                aligned,
            },
            scissor: Some(scissor),
            bounds: scissor,
        }
    }

    /// Target pixels the draw can touch.
    pub(crate) fn bounds(&self) -> (u32, u32, u32, u32) {
        self.bounds
    }

    /// Whether the draw's quads are [`TurnedGlyph`]s, or `None` for a
    /// retained run, which draws on its own.
    fn turned(&self) -> Option<bool> {
        match self.source {
            GlyphDrawSource::Shared { quads, .. } => Some(quads.turned()),
            GlyphDrawSource::Retained { .. } => None,
        }
    }
}

/// The kind of glyph draw, turned (`true`) or plain, that can move after
/// every draw of the other kind in `draws` without moving past a draw it
/// shares a pixel with: glyphs blend, so only draws that share pixels must
/// keep their order. The fewer kind is tried first, which checks fewer
/// pairs. `None` when neither can move, a retained run is among them, or
/// the kinds do not change back and forth.
pub(crate) fn movable_glyph_kind<T>(
    draws: &[T],
    kind: impl Fn(&T) -> Option<bool>,
    bounds: impl Fn(&T) -> TargetRect,
) -> Option<bool> {
    let mut turned = 0;
    for draw in draws {
        turned += usize::from(kind(draw)?);
    }
    let changes = draws
        .windows(2)
        .filter(|pair| kind(&pair[0]) != kind(&pair[1]))
        .count();
    if changes < 2 {
        return None;
    }
    let fewer = turned * 2 < draws.len();
    [fewer, !fewer].into_iter().find(|&moved| {
        draws.iter().enumerate().all(|(index, draw)| {
            kind(draw) != Some(moved)
                || draws[index + 1..].iter().all(|later| {
                    kind(later) == Some(moved)
                        || !crate::draw_pass::target_rects_overlap(bounds(draw), bounds(later))
                })
        })
    })
}

/// Moves a glyph batch's retained runs after its shared quads, each in its
/// order, when no run shares a pixel with a shared draw after it: the
/// shared quads around a run, which follow each other in the frame's
/// buffer, then draw at once. A card's body text, a retained run between
/// its title and its labels, split them into two draws. Returns how many
/// shared draws lead the batch, none when its runs keep their places;
/// `moved` is scratch.
pub(crate) fn defer_retained_glyph_runs(
    cmds: &mut [GlyphDrawCmd],
    moved: &mut Vec<GlyphDrawCmd>,
) -> usize {
    let retained = |cmd: &GlyphDrawCmd| cmd.turned().is_none();
    let shared = cmds.iter().filter(|cmd| !retained(cmd)).count();
    if cmds[..shared].iter().all(|cmd| !retained(cmd)) {
        return shared;
    }
    let deferrable = cmds.iter().enumerate().all(|(index, cmd)| {
        !retained(cmd)
            || cmds[index + 1..].iter().all(|later| {
                retained(later)
                    || !crate::draw_pass::target_rects_overlap(cmd.bounds(), later.bounds())
            })
    });
    if !deferrable {
        return 0;
    }
    moved.clear();
    let mut kept = 0;
    for read in 0..cmds.len() {
        if retained(&cmds[read]) {
            moved.push(cmds[read].clone());
        } else {
            cmds.swap(kept, read);
            kept += 1;
        }
    }
    for (slot, cmd) in cmds[kept..].iter_mut().zip(moved.drain(..)) {
        *slot = cmd;
    }
    shared
}

/// Moves a glyph batch's draws of the kind [`movable_glyph_kind`] picks
/// after those of the other kind, each kind in its order, so each draws
/// once instead of once per change between them. `moved` is scratch.
pub(crate) fn group_glyph_kinds(cmds: &mut [GlyphDrawCmd], moved: &mut Vec<GlyphDrawCmd>) {
    let Some(kind) = movable_glyph_kind(cmds, GlyphDrawCmd::turned, GlyphDrawCmd::bounds) else {
        return;
    };
    moved.clear();
    let mut kept = 0;
    for read in 0..cmds.len() {
        if cmds[read].turned() == Some(kind) {
            moved.push(cmds[read].clone());
        } else {
            cmds.swap(kept, read);
            kept += 1;
        }
    }
    for (slot, cmd) in cmds[kept..].iter_mut().zip(moved.drain(..)) {
        *slot = cmd;
    }
}

pub(super) fn glyph_instance_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex("Glyph Instance Buffer", size_of::<GlyphInstance>() as u64)
}

pub(super) fn turned_glyph_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex(
        "Turned Glyph Instance Buffer",
        size_of::<TurnedGlyph>() as u64,
    )
}

#[cfg(test)]
#[path = "tests/glyph_kind_tests.rs"]
mod tests;
