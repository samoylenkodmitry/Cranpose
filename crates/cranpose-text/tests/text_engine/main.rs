// The text engine on the repository's fonts.

mod default_tracking;
mod font_family_resolver;
mod font_feature;
mod fonts;
mod glyph_outline;
mod harfrust_oracle;
#[cfg(feature = "hyphenation-embedded")]
mod hyphenation;
mod intrinsics;
mod itemizer;
mod line_break;
mod paragraph_lengths;
mod paragraph_width;
mod plain_words;
mod shaper;
mod text_layout;
mod text_overflow;
mod text_queries;
mod typeface;
mod width_hold;
mod word_cache;
