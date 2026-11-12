use crate::{
    render::{GlyphDrawCmd, TargetRect, ViewportUniformParams},
    scissor::target_rects_overlap,
};

/// Most glyph and image draws held back at once; past this they draw, so a
/// long stretch of shapes checks each against a bounded list.
const MAX_HELD_DRAWS: usize = 256;

fn target_rect_union(a: TargetRect, b: TargetRect) -> TargetRect {
    let left = a.0.min(b.0);
    let top = a.1.min(b.1);
    let right = (a.0 + a.2).max(b.0 + b.2);
    let bottom = (a.1 + a.3).max(b.1 + b.3);
    (left, top, right - left, bottom - top)
}

/// Log2 of the side, in target pixels, of the cells [`HeldDraws`] marks.
const HELD_CELL_SHIFT: u32 = 6;

/// The rows of cells `rect` touches and the mask of its columns in each,
/// columns past the 63rd sharing the last bit. An empty side counts as one
/// pixel: [`target_rects_overlap`] lets a rect that thin overlap one it lies
/// inside, and the pixel it stands on is in that one too.
pub(super) fn held_cells(rect: TargetRect) -> (std::ops::Range<usize>, u64) {
    let (x, y, width, height) = rect;
    let last_column = (x.saturating_add(width.max(1) - 1) >> HELD_CELL_SHIFT).min(63);
    let first_column = (x >> HELD_CELL_SHIFT).min(63);
    let columns = last_column - first_column + 1;
    let mask = if columns >= 64 {
        u64::MAX
    } else {
        ((1_u64 << columns) - 1) << first_column
    };
    let first_row = (y >> HELD_CELL_SHIFT) as usize;
    let last_row = (y.saturating_add(height.max(1) - 1) >> HELD_CELL_SHIFT) as usize;
    (first_row..last_row + 1, mask)
}

// The target pixels one lane of held draws touches.
#[derive(Default)]
struct LaneCells {
    union: Option<TargetRect>,
    // Per band of cell rows, the cell columns a held draw touches. A shape
    // touching none of them overlaps no held draw.
    cells: Vec<u64>,
    // Per band of cell rows, the target pixels of each held draw touching
    // it: a shape that touches a held cell is checked only against the
    // draws of its own bands. A list's tickers held a few hundred glyph
    // draws, which every background after them was checked against.
    rows: Vec<Vec<TargetRect>>,
}

impl LaneCells {
    fn mark(&mut self, rect: TargetRect) {
        self.union = Some(
            self.union
                .map_or(rect, |union| target_rect_union(union, rect)),
        );
        let (rows, mask) = held_cells(rect);
        if self.cells.len() < rows.end {
            self.cells.resize(rows.end, 0);
        }
        if self.rows.len() < rows.end {
            self.rows.resize_with(rows.end, Vec::new);
        }
        for (cells, held) in self.cells[rows.clone()]
            .iter_mut()
            .zip(&mut self.rows[rows])
        {
            *cells |= mask;
            held.push(rect);
        }
    }

    fn is_empty(&self) -> bool {
        self.union.is_none()
    }

    // Whether a draw touching `rect` would cover a held draw: one that
    // overlaps it shares a band of cell rows with it.
    fn overlaps(&self, rect: TargetRect) -> bool {
        if !self
            .union
            .is_some_and(|union| target_rects_overlap(union, rect))
        {
            return false;
        }
        let (rows, mask) = held_cells(rect);
        let end = rows.end.min(self.cells.len());
        let start = rows.start.min(end);
        self.cells[start..end]
            .iter()
            .zip(&self.rows[start..end])
            .any(|(cells, held)| {
                cells & mask != 0 && held.iter().any(|held| target_rects_overlap(*held, rect))
            })
    }

    fn clear(&mut self) {
        for held in &mut self.rows[..self.cells.len()] {
            held.clear();
        }
        self.cells.clear();
        self.union = None;
    }
}

// Where a shape goes among the held draws.
pub(super) enum ShapeLane {
    // Into the open chunk: it overlaps no held draw.
    Chunk,
    // Held behind the glyphs and images it overlaps, within these pixels.
    Behind(TargetRect),
    // After everything held is drawn.
    Flush,
}

// Where the glyph draws of a text go among the held draws.
#[derive(PartialEq)]
pub(super) enum GlyphLane {
    Front,
    Behind,
    Flush,
}

/// Glyph and src-over image draws held back so the shapes after them keep
/// filling one arena chunk: the text and avatar of a card no longer split
/// the backgrounds around them into draws of their own. The held draws
/// lie in lanes, drawn in this order after the chunk's shapes so far: glyphs
/// and images in front, then the shapes behind them, then glyphs behind
/// those. An item joins the first lane of its kind that no later lane holds
/// a draw it overlaps; one that fits none draws everything held first, so
/// nothing is reordered past a pixel it shares. The glyphs in front draw
/// before the images, so an image may lie over held glyphs and a text that
/// lies over a held image goes behind the shapes.
#[derive(Default)]
pub(crate) struct HeldDraws {
    cmds: Option<std::ops::Range<usize>>,
    /// The held image commands and the viewport they draw under.
    images: Option<(std::ops::Range<usize>, ViewportUniformParams)>,
    /// How many draws are held.
    held: usize,
    /// The target pixels the held images touch.
    image_bounds: Vec<TargetRect>,
    front: LaneCells,
    shapes: LaneCells,
    glyphs: LaneCells,
}

impl HeldDraws {
    /// Holds the glyph commands at `cmds`, which touch `bounds`.
    pub(super) fn hold(
        &mut self,
        cmds: std::ops::Range<usize>,
        bounds: impl IntoIterator<Item = TargetRect>,
    ) {
        self.cmds = Some(match self.cmds.take() {
            Some(held) => held.start..cmds.end,
            None => cmds,
        });
        for rect in bounds {
            self.front.mark(rect);
            self.held += 1;
        }
    }

    /// Holds the image command at `cmds`, drawn under `viewport` within
    /// `bounds`.
    pub(super) fn hold_image(
        &mut self,
        cmds: std::ops::Range<usize>,
        viewport: ViewportUniformParams,
        bounds: TargetRect,
    ) {
        let cmds = match self.images.take() {
            Some((held, _)) => held.start..cmds.end,
            None => cmds,
        };
        self.images = Some((cmds, viewport));
        self.image_bounds.push(bounds);
        self.front.mark(bounds);
        self.held += 1;
    }

    // Holds a run of shapes that touches `reach` behind the glyphs and images.
    pub(super) fn hold_shapes(&mut self, reach: TargetRect) {
        self.shapes.mark(reach);
        self.held += 1;
    }

    // Holds glyph draws `cmds` behind the held shapes.
    pub(super) fn hold_behind(&mut self, cmds: &[GlyphDrawCmd]) {
        for cmd in cmds {
            self.glyphs.mark(cmd.bounds());
            self.held += 1;
        }
    }

    /// Whether nothing is held.
    pub(super) fn is_empty(&self) -> bool {
        self.front.is_empty() && self.shapes.is_empty() && self.glyphs.is_empty()
    }

    /// Whether a draw touching `rect` would cover a held draw.
    pub(super) fn overlaps(&self, rect: TargetRect) -> bool {
        self.front.overlaps(rect) || self.shapes.overlaps(rect) || self.glyphs.overlaps(rect)
    }

    // Where a run of shapes that can reach `reach` goes. It goes behind the
    // held draws it overlaps unless it overlaps held glyphs that draw after
    // its lane.
    pub(super) fn place_shapes(&self, reach: Option<TargetRect>) -> ShapeLane {
        let Some(reach) = reach.filter(|reach| self.overlaps(*reach)) else {
            return ShapeLane::Chunk;
        };
        if self.full() || self.glyphs.overlaps(reach) {
            ShapeLane::Flush
        } else {
            ShapeLane::Behind(reach)
        }
    }

    // Where the glyph draws `cmds` of a text go: behind the shapes when they
    // overlap held images or shapes, or glyphs behind those, and the text
    // can wait there; else after everything held is drawn.
    pub(super) fn place_glyphs(&self, cmds: &[GlyphDrawCmd]) -> GlyphLane {
        if self.full() {
            return GlyphLane::Flush;
        }
        let behind = cmds.iter().any(|cmd| {
            let rect = cmd.bounds();
            self.covers_image(rect) || self.shapes.overlaps(rect) || self.glyphs.overlaps(rect)
        });
        match (behind, cmds.iter().all(GlyphDrawCmd::takes_shared_plain)) {
            (false, _) => GlyphLane::Front,
            (true, true) => GlyphLane::Behind,
            (true, false) => GlyphLane::Flush,
        }
    }

    // Whether a draw touching `rect` overlaps one held behind the images.
    pub(super) fn overlaps_behind(&self, rect: TargetRect) -> bool {
        self.shapes.overlaps(rect) || self.glyphs.overlaps(rect)
    }

    /// Whether glyphs touching `rect` would cover a held image.
    pub(super) fn covers_image(&self, rect: TargetRect) -> bool {
        self.image_bounds
            .iter()
            .any(|held| target_rects_overlap(*held, rect))
    }

    /// The viewport the held images draw under.
    pub(super) fn image_viewport(&self) -> Option<ViewportUniformParams> {
        self.images.as_ref().map(|(_, viewport)| *viewport)
    }

    pub(super) fn full(&self) -> bool {
        self.held >= MAX_HELD_DRAWS
    }

    // Whether runs or glyphs are held behind the glyphs and images.
    pub(super) fn has_behind(&self) -> bool {
        !self.shapes.is_empty() || !self.glyphs.is_empty()
    }

    pub(super) fn take(&mut self) -> HeldCmds {
        let behind = self.has_behind();
        self.held = 0;
        self.image_bounds.clear();
        self.front.clear();
        self.shapes.clear();
        self.glyphs.clear();
        HeldCmds {
            glyphs: self.cmds.take(),
            images: self.images.take(),
            behind,
        }
    }
}

/// The commands [`HeldDraws::take`] hands back to draw.
pub(super) struct HeldCmds {
    pub(super) glyphs: Option<std::ops::Range<usize>>,
    pub(super) images: Option<(std::ops::Range<usize>, ViewportUniformParams)>,
    // Whether runs or glyphs are held behind them.
    pub(super) behind: bool,
}
