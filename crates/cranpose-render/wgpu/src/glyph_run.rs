//! The glyphs of a text run the renderer keeps between frames.

use std::rc::Rc;

use cranpose_render_common::software_text_raster::{
    SoftwareGlyphAtlasKey, SoftwareGlyphAtlasPlacement,
};
use cranpose_ui_graphics::Color;

/// What the glyphs of one segment of a run share: face, size and synthesis
/// (an atlas key but for its glyph id) and color.
#[derive(Clone, Copy, PartialEq)]
struct RunFace {
    key: SoftwareGlyphAtlasKey,
    color: Color,
}

impl RunFace {
    fn of(glyph: &SoftwareGlyphAtlasPlacement) -> Self {
        Self {
            key: SoftwareGlyphAtlasKey {
                glyph_id: 0,
                ..glyph.key
            },
            color: glyph.color,
        }
    }
}

/// A glyph of a run: where it sits in the run's raster, its size, its
/// glyph id and the index of its face.
#[derive(Clone, Copy)]
struct RunGlyph {
    x: i32,
    y: i32,
    width: u16,
    height: u16,
    glyph_id: u16,
    face: u16,
}

impl RunGlyph {
    fn of(glyph: &SoftwareGlyphAtlasPlacement, face: usize) -> Option<Self> {
        Some(Self {
            x: glyph.x,
            y: glyph.y,
            width: u16::try_from(glyph.width).ok()?,
            height: u16::try_from(glyph.height).ok()?,
            glyph_id: u16::try_from(glyph.key.glyph_id).ok()?,
            face: u16::try_from(face).ok()?,
        })
    }
}

/// A run's drawing glyphs, 16 bytes each where a placement takes 72: what
/// a segment's glyphs share is held once, as their face.
#[derive(Clone)]
pub(crate) struct RunGlyphs {
    faces: Rc<[RunFace]>,
    glyphs: Rc<[RunGlyph]>,
}

/// Buffers a renderer reuses while it builds runs, and the faces of the
/// last run built, which the next run shares when its own are the same.
#[derive(Default)]
pub(crate) struct RunGlyphScratch {
    faces: Vec<RunFace>,
    glyphs: Vec<RunGlyph>,
    last_faces: Rc<[RunFace]>,
}

impl RunGlyphs {
    /// The run of `placements`, sharing the last run's faces when they are
    /// the same, as consecutive texts in one style's are. `None` when a
    /// glyph has no compact form: a glyph id or a mask side past 65535, or
    /// as many faces.
    pub(crate) fn of(
        placements: impl Iterator<Item = SoftwareGlyphAtlasPlacement>,
        scratch: &mut RunGlyphScratch,
    ) -> Option<Self> {
        scratch.faces.clear();
        scratch.glyphs.clear();
        for placement in placements {
            let face = RunFace::of(&placement);
            if scratch.faces.last() != Some(&face) {
                scratch.faces.push(face);
            }
            let glyph = RunGlyph::of(&placement, scratch.faces.len() - 1)?;
            scratch.glyphs.push(glyph);
        }
        if *scratch.last_faces != *scratch.faces {
            scratch.last_faces = Rc::from(scratch.faces.as_slice());
        }
        Some(Self {
            faces: Rc::clone(&scratch.last_faces),
            glyphs: Rc::from(scratch.glyphs.as_slice()),
        })
    }

    /// Each glyph as the placement it was made from.
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = SoftwareGlyphAtlasPlacement> + '_ {
        self.glyphs.iter().map(|glyph| {
            let face = self.faces[usize::from(glyph.face)];
            SoftwareGlyphAtlasPlacement {
                key: SoftwareGlyphAtlasKey {
                    glyph_id: u32::from(glyph.glyph_id),
                    ..face.key
                },
                x: glyph.x,
                y: glyph.y,
                width: usize::from(glyph.width),
                height: usize::from(glyph.height),
                color: face.color,
            }
        })
    }
}

#[cfg(test)]
#[path = "tests/glyph_run_tests.rs"]
mod tests;
