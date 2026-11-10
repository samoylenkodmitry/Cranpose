//! Text shaped once and drawn by the views that paint themselves.

use std::ops::Range;

use floem::{
    peniko::Color,
    text::{Attrs, AttrsList, FONT_SYSTEM, FamilyOwned, LineHeightValue, TextLayout, Weight},
};

pub const ROBOTO: &str = "Roboto";
pub const MONO: &str = "DejaVu Sans Mono";

/// Which face a text is set in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Regular,
    Bold,
    Mono,
}

impl Face {
    fn family(self) -> FamilyOwned {
        FamilyOwned::Name(
            match self {
                Self::Mono => MONO,
                Self::Regular | Self::Bold => ROBOTO,
            }
            .to_owned(),
        )
    }

    fn weight(self) -> Weight {
        match self {
            Self::Bold => Weight::BOLD,
            Self::Regular | Self::Mono => Weight::NORMAL,
        }
    }
}

/// Adds the font files every desktop app loads to the font database.
pub fn load_fonts(files: &[&str]) {
    let mut system = FONT_SYSTEM.lock();
    for file in files {
        match perf_data::font_bytes(file) {
            Ok(bytes) => system.db_mut().load_font_data(bytes),
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
}

/// The look of a run of text.
#[derive(Clone, Copy, Debug)]
pub struct Type {
    pub size: f32,
    /// The line height as a multiple of the size.
    pub line: f32,
    pub face: Face,
    pub color: Color,
}

impl Type {
    pub const fn new(size: f32, line: f32, face: Face, color: Color) -> Self {
        Self {
            size,
            line,
            face,
            color,
        }
    }

    /// The height of one line.
    pub fn line_height(self) -> f32 {
        self.size * self.line
    }

    fn attrs<'a>(self, family: &'a [FamilyOwned], color: Color) -> Attrs<'a> {
        self.attrs_in(self.face, family, color)
    }

    fn attrs_in<'a>(self, face: Face, family: &'a [FamilyOwned], color: Color) -> Attrs<'a> {
        Attrs::new()
            .family(family)
            .weight(face.weight())
            .color(color)
            .font_size(self.size)
            .line_height(LineHeightValue::Normal(self.line))
    }

    /// `text` on one line, or wrapped at `width`.
    pub fn shape(self, text: &str, width: Option<f32>) -> TextLayout {
        self.shape_spans(text, &[], width)
    }

    /// `text` with the colors of `spans` over the type's own.
    pub fn shape_spans(
        self,
        text: &str,
        spans: &[(Range<usize>, Color)],
        width: Option<f32>,
    ) -> TextLayout {
        let family = [self.face.family()];
        let mut list = AttrsList::new(self.attrs(&family, self.color));
        for (range, color) in spans {
            list.add_span(range.clone(), self.attrs(&family, *color));
        }
        let mut layout = TextLayout::new();
        layout.set_text(text, list);
        if let Some(width) = width {
            layout.set_size(width, f32::MAX);
        }
        layout
    }

    /// One line of runs `(text, bold, color)` set one after another.
    pub fn shape_runs(self, runs: &[(&str, bool, Color)]) -> TextLayout {
        let family = [self.face.family()];
        let text: String = runs.iter().map(|(text, ..)| *text).collect();
        let mut list = AttrsList::new(self.attrs(&family, self.color));
        let mut start = 0;
        for (run, bold, color) in runs {
            let face = if *bold { Face::Bold } else { self.face };
            list.add_span(
                start..start + run.len(),
                self.attrs_in(face, &family, *color),
            );
            start += run.len();
        }
        let mut layout = TextLayout::new();
        layout.set_text(&text, list);
        layout
    }
}
