// The background modifier and the corner shape it draws in: plain elements,
// which a chain keeps as words.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{NodeCapabilities, PlainElement};

use super::hash_f32_value;
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

impl Hash for BackgroundElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.color.0);
        hash_f32_value(state, self.color.1);
        hash_f32_value(state, self.color.2);
        hash_f32_value(state, self.color.3);
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

impl Hash for CornerShapeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let radii = self.shape.radii();
        hash_f32_value(state, radii.top_left);
        hash_f32_value(state, radii.top_right);
        hash_f32_value(state, radii.bottom_right);
        hash_f32_value(state, radii.bottom_left);
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
