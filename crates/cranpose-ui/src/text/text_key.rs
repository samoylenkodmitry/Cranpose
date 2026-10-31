use std::hash::{Hash, Hasher};

use super::{AnnotatedString, layout_input::TextUnits, layout_options::TextLayoutOptions};

// What a layout is cached by: the text and style hashes, the options and the
// units its sizes became pixels in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TextKey {
    text_hash: u64,
    style_hash: u64,
    options: TextLayoutOptions,
    // The density's bits, the font scale curve's fingerprint and the size
    // factor's bits.
    units: (u32, u32, u32),
}

impl TextKey {
    // The key of `text` in `style` by `style_hash`; `None` for a text with
    // annotations, which a layout of another text with the same look must
    // not stand in for.
    pub(super) fn of(
        text: &AnnotatedString,
        style_hash: u64,
        options: TextLayoutOptions,
        units: TextUnits,
    ) -> Option<Self> {
        (text.string_annotations.is_empty() && text.link_annotations.is_empty()).then(|| Self {
            text_hash: text.render_hash(),
            style_hash,
            options: options.normalized(),
            units: (
                units.density.density().to_bits(),
                units.density.font_scale_curve().fingerprint(),
                units.factor.to_bits(),
            ),
        })
    }

    // The text's and the style's hashes in one, mixed into the high bits.
    // The options and units, which an app's texts rarely differ in, are left
    // to equality.
    pub(super) fn mixed_hash(&self) -> u64 {
        (self.text_hash ^ self.style_hash.rotate_left(32)).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }
}

impl Hash for TextKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.mixed_hash());
    }
}
