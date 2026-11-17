use std::ops::{Deref, DerefMut};

use cranpose_ui_graphics::MemoryHeadroom;

use crate::shaped_glyphs::ShapedGlyphs;

// The most glyphs the scratch keeps room for whatever the texts need: all it
// keeps between texts when memory is tight, and while the texts rest.
const KEPT_GLYPHS: usize = 4096;

// The glyphs of the text being shaped, which keep their storage from one
// text to the next: through a layout pass, and into the next while the last
// pass needed half of it, so a long paragraph laid out each frame shapes in
// one storage.
#[derive(Default)]
pub(crate) struct GlyphScratch {
    glyphs: ShapedGlyphs,
    // The most room a text asked for since the pass began.
    needed: usize,
    tight: bool,
}

impl GlyphScratch {
    // Makes room for `additional` glyphs, which a text of that many bytes
    // takes at most.
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.glyphs.reserve(additional);
        self.needed = self.needed.max(additional);
    }

    // A text is laid out: tight memory takes back the room past the kept.
    pub(crate) fn end_text(&mut self) {
        if self.tight {
            self.give_back_past(0);
        }
    }

    // A layout pass begins with `headroom` memory left: the room past twice
    // what the last pass needed goes, and all past the kept when memory is
    // tight.
    pub(crate) fn begin_pass(&mut self, headroom: MemoryHeadroom) {
        self.tight = headroom == MemoryHeadroom::Tight;
        let needed = std::mem::take(&mut self.needed).saturating_mul(2);
        self.give_back_past(if self.tight { 0 } else { needed });
    }

    // The texts rest: the room past the kept goes.
    pub(crate) fn rest(&mut self) {
        self.needed = 0;
        self.give_back_past(0);
    }

    // Gives back the storage when it holds room for more than `needed`
    // glyphs and more than the kept.
    fn give_back_past(&mut self, needed: usize) {
        let room = self.glyphs.capacity();
        if room > KEPT_GLYPHS && room > needed {
            self.glyphs = ShapedGlyphs::default();
        }
    }
}

impl Deref for GlyphScratch {
    type Target = ShapedGlyphs;

    fn deref(&self) -> &ShapedGlyphs {
        &self.glyphs
    }
}

impl DerefMut for GlyphScratch {
    fn deref_mut(&mut self) -> &mut ShapedGlyphs {
        &mut self.glyphs
    }
}
