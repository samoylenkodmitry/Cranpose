// The background modifier and the corner shape it draws in: plain elements,
// which a chain keeps as words.

use cranpose_foundation::{NodeCapabilities, PlainElement};

use crate::modifier::{Color, RoundedCornerShape};

/// Element of a background: a fill behind the node's content.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundElement {
    color: Color,
}

impl BackgroundElement {
    /// Creates an element that fills its node's bounds with `color`.
    pub fn new(color: Color) -> Self {
        Self { color }
    }

    /// Returns the fill color.
    pub fn color(&self) -> Color {
        self.color
    }
}

impl PlainElement for BackgroundElement {
    const WORDS: usize = 4;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::DRAW;

    fn to_words(&self, words: &mut [u32]) {
        let Color(red, green, blue, alpha) = self.color;
        words.copy_from_slice(&[red, green, blue, alpha].map(f32::to_bits));
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let channels = match *words {
            [red, green, blue, alpha, ..] => [red, green, blue, alpha],
            _ => [0; 4],
        };
        let [red, green, blue, alpha] = channels.map(f32::from_bits);
        Self {
            color: Color(red, green, blue, alpha),
        }
    }
}

/// Element of the rounded corner shape a node's backgrounds draw in.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerShapeElement {
    shape: RoundedCornerShape,
}

impl CornerShapeElement {
    /// Creates an element that holds `shape` as the current rounded corner shape.
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self { shape }
    }

    /// Returns the corner shape.
    pub fn shape(&self) -> RoundedCornerShape {
        self.shape
    }
}

impl PlainElement for CornerShapeElement {
    const WORDS: usize = 4;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::DRAW;

    fn to_words(&self, words: &mut [u32]) {
        let radii = self.shape.radii();
        words.copy_from_slice(
            &[
                radii.top_left,
                radii.top_right,
                radii.bottom_right,
                radii.bottom_left,
            ]
            .map(f32::to_bits),
        );
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let radii = match *words {
            [top_left, top_right, bottom_right, bottom_left, ..] => {
                [top_left, top_right, bottom_right, bottom_left]
            }
            _ => [0; 4],
        };
        let [top_left, top_right, bottom_right, bottom_left] = radii.map(f32::from_bits);
        Self {
            shape: RoundedCornerShape::new(top_left, top_right, bottom_right, bottom_left),
        }
    }
}
