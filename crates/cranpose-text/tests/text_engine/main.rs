// The text engine on the repository's fonts.

mod font_family_resolver;
mod font_feature;
mod fonts;
mod glyph_outline;
#[cfg(feature = "hyphenation-embedded")]
mod hyphenation;
mod intrinsics;
mod itemizer;
mod line_break;
mod paragraph_lengths;
mod paragraph_width;
mod shaper;
mod text_layout;
mod text_overflow;
mod text_queries;
mod typeface;
mod width_hold;
mod word_cache;
