//! Colours and text styles a Wear widget is handed.
//!
//! Cranpose has no theme system, and this module does not start one. There is
//! no composition local, no `MaterialTheme`, no ambient lookup: every Wear
//! widget takes a [`WearColors`] in its spec and a caller passes one down.
//! Building a theme system is a much larger design than a widget set, and the
//! widgets do not need it.
//!
//! What is worth encoding is the part a port gets wrong from reading the Kotlin:
//! which role each slot draws with. `MaterialTheme` provides eight composition
//! locals and `LocalContentColor` is **not** among them — its declaration is
//! `compositionLocalOf { Color.White }` and only `AppScaffold` overrides it. So
//! a bare `Text` on a Wear screen is white, while a `ListHeader` on the same
//! screen is `onBackground`. Two different whites side by side, and a port that
//! resolves both to `onBackground` is wrong by 8 counts of red and 9 of green
//! on every bare `Text`. [`WearColors::content`] is that colour, kept separate
//! for exactly that reason.

use crate::{
    modifier::Color,
    text::{
        paragraph::TextAlign,
        style::{
            LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim, ParagraphStyle,
            PlatformParagraphStyle, SpanStyle, TextStyle,
        },
        FontFamily, FontWeight, TextUnit,
    },
    widgets::wear::color_appearance::set_luminance,
};

/// The colour roles these widgets read.
///
/// This is the subset of Wear Material 3's `ColorScheme` that the Settings and
/// Credits screens reach, not the whole scheme — a role nothing draws with is
/// a role nobody can get wrong.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearColors {
    /// A filled `Button`'s container, and a checked switch's track.
    pub primary: Color,
    /// A checked `SwitchButton`'s container, and its thumb.
    pub primary_container: Color,
    /// A filled `Button`'s label.
    pub on_primary: Color,
    /// A checked `SwitchButton`'s label.
    pub on_primary_container: Color,
    /// An unchecked `SwitchButton`'s container and track.
    pub surface_container: Color,
    pub on_surface: Color,
    /// A secondary label on an unchecked row.
    pub on_surface_variant: Color,
    /// An unchecked switch's track border and thumb.
    pub outline: Color,
    pub background: Color,
    /// A `ListHeader`'s label.
    pub on_background: Color,
    /// `LocalContentColor`, which `MaterialTheme` does not provide and which is
    /// therefore plain white unless an `AppScaffold` says otherwise. A bare
    /// `Text` in the list draws with this, **not** with `on_background`.
    pub content: Color,
    /// The scroll indicator's thumb: `onBackground` taken to L\* 80.
    ///
    /// Derive it with [`WearColors::with_wear_scroll_indicator`] rather than
    /// picking it — see that method for what Wear's own derivation is and why
    /// the obvious substitute for it is wrong.
    pub indicator_thumb: Color,
    /// The scroll indicator's track: the same colour at L\* 20.
    pub indicator_track: Color,
}

impl Default for WearColors {
    /// A plain dark scheme. It is not any app's palette — a caller with a
    /// palette passes it in.
    fn default() -> Self {
        Self {
            primary: Color::from_rgb_u8(0xA8, 0xC7, 0xFA),
            primary_container: Color::from_rgb_u8(0x0B, 0x57, 0xD0),
            on_primary: Color::from_rgb_u8(0x00, 0x00, 0x00),
            on_primary_container: Color::from_rgb_u8(0xD3, 0xE3, 0xFD),
            surface_container: Color::from_rgb_u8(0x1E, 0x1F, 0x20),
            on_surface: Color::from_rgb_u8(0xE3, 0xE3, 0xE3),
            on_surface_variant: Color::from_rgb_u8(0xC4, 0xC7, 0xC5),
            outline: Color::from_rgb_u8(0x8E, 0x91, 0x8F),
            background: Color::from_rgb_u8(0x00, 0x00, 0x00),
            on_background: Color::from_rgb_u8(0xE3, 0xE3, 0xE3),
            content: Color::WHITE,
            // Not written out: the scheme's own `on_background` through the
            // rule below, so the default cannot say something the derivation
            // does not.
            indicator_thumb: Color::WHITE,
            indicator_track: Color::WHITE,
        }
        .with_wear_scroll_indicator()
    }
}

impl WearColors {
    /// The two scroll-indicator colours Wear derives from this scheme's
    /// `on_background`.
    ///
    /// `ScrollIndicatorDefaults.colors()` reads one token and moves it to two
    /// lightnesses: `setLuminance(fromToken(OnBackground), 80f)` for the thumb
    /// and `setLuminance(..., 20f)` for the track. Nothing else feeds it — not
    /// `outline`, not an alpha over the background — and both come from
    /// `onBackground` rather than from `onSurface` or `primary`.
    ///
    /// The move itself is a **CAM16** round trip, not a CIE L\*a\*b\* one; see
    /// [`crate::widgets::wear::color_appearance::set_luminance`] for the two
    /// models' disagreement and why substituting L\* in Lab passes a check
    /// against the thumb and fails against the track.
    ///
    /// Overriding either afterwards is what
    /// `ScrollIndicatorDefaults.colors(indicatorColor, trackColor)` does: it
    /// copies over the derived pair.
    pub fn with_wear_scroll_indicator(mut self) -> Self {
        self.indicator_thumb = set_luminance(self.on_background, 80.0);
        self.indicator_track = set_luminance(self.on_background, 20.0);
        self
    }
}

/// Wear's `DefaultTextStyle` paragraph policy: no font padding, the leading
/// centred, nothing trimmed — and the font's own extent as a floor, which is
/// the part that makes a 16sp/18sp style lay out in 38 pixels rather than 36.
pub fn wear_line_height_style() -> LineHeightStyle {
    LineHeightStyle {
        alignment: LineHeightAlignment::Center,
        trim: LineHeightTrim::None,
        mode: LineHeightMode::Minimum,
    }
}

/// One of Wear Material 3's type-scale entries.
///
/// `size` and `line_height` are in sp; `tracking` is letter spacing in sp.
/// `weight` is both the `FontWeight` and the `wght` variation axis — the tokens
/// set them to the same number.
///
/// `align` is not part of Wear's type scale — every token leaves it unset and a
/// call site states it. It lives here anyway because the alternative is for
/// every caller to reach into the resolved [`TextStyle`]'s paragraph style and
/// overwrite one field, which is how a type scale stops being the thing that
/// describes the text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearTextStyle {
    pub size_sp: f32,
    pub line_height_sp: f32,
    pub weight: u16,
    pub tracking_sp: f32,
    /// `TextAlign::Unspecified` on every scale entry, as in the tokens.
    pub align: TextAlign,
}

impl WearTextStyle {
    /// The family Wear's type scale resolves to on a real device.
    ///
    /// Wear's `TypefaceTokens.Brand` is `DeviceFontFamilyName("roboto-flex")`,
    /// and naming that here would be the faithful-looking answer and the wrong
    /// one. On the Wear OS 5 system image these widgets are measured against,
    /// `/system/etc/fonts.xml` declares a `roboto-flex` family whose every entry
    /// points at `RobotoFlex-Regular.ttf` — **a file the image does not ship**.
    /// A family whose files cannot be opened is dropped, so the token does not
    /// resolve and the platform falls back to `sans-serif`, which is Roboto.
    /// That is what the pixels show, and it is what this names.
    ///
    /// Naming a family at all is not optional. A `TextStyle` that leaves
    /// `font_family` as `None` only draws if some face happens to answer for the
    /// default, and an app that registers the system fonts under their own
    /// families — which is what a port matching Android's text has to do — has
    /// no such face. The text then measures, lays out and rasterises to nothing:
    /// a screen with correct geometry and no glyphs on it.
    pub const BRAND_FAMILY: FontFamily = FontFamily::SansSerif;

    /// `titleMedium` — what a `ListHeader` draws with.
    pub const TITLE_MEDIUM: Self = Self {
        size_sp: 16.0,
        line_height_sp: 18.0,
        weight: 550,
        tracking_sp: 0.4,
        align: TextAlign::Unspecified,
    };
    /// `labelMedium` — a `Button` label and a `SwitchButton` label.
    pub const LABEL_MEDIUM: Self = Self {
        size_sp: 15.0,
        line_height_sp: 18.0,
        weight: 500,
        tracking_sp: 0.4,
        align: TextAlign::Unspecified,
    };
    /// `labelSmall` — a secondary label.
    pub const LABEL_SMALL: Self = Self {
        size_sp: 13.0,
        line_height_sp: 16.0,
        weight: 500,
        tracking_sp: 0.4,
        align: TextAlign::Unspecified,
    };
    /// `bodyLarge` — the theme default, which a bare `Text` inherits. Note that
    /// a bare `Text` that overrides only its `fontSize` keeps **this** line
    /// height: a 12sp glyph in an 18sp line box is a real Wear screen, not a
    /// mistake to unify away.
    pub const BODY_LARGE: Self = Self {
        size_sp: 16.0,
        line_height_sp: 18.0,
        weight: 450,
        tracking_sp: 0.4,
        align: TextAlign::Unspecified,
    };

    /// The same style at another glyph size, keeping the line height.
    ///
    /// This is the shape of Wear's `Text(text, fontSize = 12.sp)` — an override
    /// of the size alone.
    pub const fn at_size(self, size_sp: f32) -> Self {
        Self { size_sp, ..self }
    }

    /// The same style with its line height stated outright.
    pub const fn with_line_height(self, line_height_sp: f32) -> Self {
        Self {
            line_height_sp,
            ..self
        }
    }

    /// The same style aligned in its own width.
    ///
    /// Wear's own `Text(text, textAlign = TextAlign.Center)` — the shape every
    /// credit line and every centred blurb on a watch screen takes.
    pub const fn aligned(self, align: TextAlign) -> Self {
        Self { align, ..self }
    }

    /// The Cranpose [`TextStyle`] this entry resolves to.
    ///
    /// The sizes stay in `Sp`, so the framework applies the user's text-size
    /// setting once, at measure time. Pre-scaling them here and handing the
    /// result to a `Text` would apply it twice.
    pub fn resolve(self, color: Color) -> TextStyle {
        self.resolve_in(color, Self::BRAND_FAMILY)
    }

    /// The same, drawn in a family the caller names.
    ///
    /// For a device whose `roboto-flex` really does resolve, or an app that
    /// registered Wear's brand face under a name of its own.
    pub fn resolve_in(self, color: Color, family: FontFamily) -> TextStyle {
        TextStyle {
            span_style: SpanStyle {
                color: Some(color),
                font_size: TextUnit::Sp(self.size_sp),
                font_weight: Some(FontWeight(self.weight)),
                letter_spacing: TextUnit::Sp(self.tracking_sp),
                font_family: Some(family),
                ..SpanStyle::default()
            },
            paragraph_style: ParagraphStyle {
                line_height: TextUnit::Sp(self.line_height_sp),
                text_align: self.align,
                line_height_style: Some(wear_line_height_style()),
                platform_style: Some(PlatformParagraphStyle {
                    include_font_padding: Some(false),
                    shaping: None,
                }),
                ..ParagraphStyle::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indicator_colours_come_from_on_background_and_nothing_else() {
        // Measured against the shipping Compose build with this palette: the
        // thumb is (180, 202, 211) and the track (30, 51, 58). A Lab round trip
        // agrees on the thumb and gives 31 of red on the track, so a scheme
        // whose track lands on 30 is the one that read Wear's own rule.
        let colors = WearColors {
            on_background: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
            ..WearColors::default()
        }
        .with_wear_scroll_indicator();
        assert_eq!(colors.indicator_thumb, Color::from_rgb_u8(180, 202, 211));
        assert_eq!(colors.indicator_track, Color::from_rgb_u8(30, 51, 58));
        // And the default scheme is derived by the same rule rather than
        // written out beside it.
        let default = WearColors::default();
        assert_eq!(default, default.with_wear_scroll_indicator());
    }

    #[test]
    fn a_bare_text_is_white_and_a_header_is_not() {
        let colors = WearColors::default();
        assert_eq!(colors.content, Color::WHITE);
        assert_ne!(colors.content, colors.on_background);
    }

    #[test]
    fn the_type_scale_carries_the_sizes_wear_declares() {
        assert_eq!(WearTextStyle::TITLE_MEDIUM.size_sp, 16.0);
        assert_eq!(WearTextStyle::TITLE_MEDIUM.line_height_sp, 18.0);
        assert_eq!(WearTextStyle::TITLE_MEDIUM.weight, 550);
        assert_eq!(WearTextStyle::LABEL_MEDIUM.size_sp, 15.0);
        assert_eq!(WearTextStyle::LABEL_SMALL.size_sp, 13.0);
        assert_eq!(WearTextStyle::LABEL_SMALL.line_height_sp, 16.0);
        assert_eq!(WearTextStyle::BODY_LARGE.weight, 450);
        for style in [
            WearTextStyle::TITLE_MEDIUM,
            WearTextStyle::LABEL_MEDIUM,
            WearTextStyle::LABEL_SMALL,
            WearTextStyle::BODY_LARGE,
        ] {
            assert_eq!(style.tracking_sp, 0.4);
        }
    }

    #[test]
    fn overriding_the_size_keeps_the_line_height_it_inherited() {
        // A 12sp glyph in an 18sp line box. Unifying the two is the quirk this
        // exists to preserve.
        let small = WearTextStyle::BODY_LARGE.at_size(12.0);
        assert_eq!(small.size_sp, 12.0);
        assert_eq!(small.line_height_sp, 18.0);
        let credit_line = small.with_line_height(16.0);
        assert_eq!(credit_line.line_height_sp, 16.0);
    }

    #[test]
    fn the_scale_itself_states_no_alignment_and_a_call_site_can() {
        // Wear's tokens set no `textAlign`; `Text(textAlign = Center)` at the
        // call site is what centres a credit line.
        for style in [
            WearTextStyle::TITLE_MEDIUM,
            WearTextStyle::LABEL_MEDIUM,
            WearTextStyle::LABEL_SMALL,
            WearTextStyle::BODY_LARGE,
        ] {
            assert_eq!(style.align, TextAlign::Unspecified);
            assert_eq!(
                style.resolve(Color::WHITE).paragraph_style.text_align,
                TextAlign::Unspecified
            );
        }
        let centred = WearTextStyle::BODY_LARGE
            .at_size(12.0)
            .with_line_height(16.0)
            .aligned(TextAlign::Center);
        assert_eq!(centred.align, TextAlign::Center);
        assert_eq!(
            centred.resolve(Color::WHITE).paragraph_style.text_align,
            TextAlign::Center
        );
        // Aligning must not disturb the rest of the entry.
        assert_eq!(centred.size_sp, 12.0);
        assert_eq!(centred.line_height_sp, 16.0);
        assert_eq!(centred.weight, WearTextStyle::BODY_LARGE.weight);
    }

    #[test]
    fn every_entry_names_a_family_so_its_text_has_a_face_to_draw_with() {
        // A `TextStyle` with no family draws only if some face answers for the
        // default. An app that registers the system fonts under their own
        // families has none, and the text then measures, lays out and
        // rasterises to nothing -- correct geometry, no glyphs.
        for style in [
            WearTextStyle::TITLE_MEDIUM,
            WearTextStyle::LABEL_MEDIUM,
            WearTextStyle::LABEL_SMALL,
            WearTextStyle::BODY_LARGE,
        ] {
            assert_eq!(
                style.resolve(Color::WHITE).span_style.font_family,
                Some(FontFamily::SansSerif),
                "Wear's brand token resolves to sans-serif on a device with no \
                 RobotoFlex-Regular.ttf, which is every Wear OS 5 image measured"
            );
        }
        assert_eq!(
            WearTextStyle::BODY_LARGE
                .resolve_in(Color::WHITE, FontFamily::Monospace)
                .span_style
                .font_family,
            Some(FontFamily::Monospace),
            "a caller can still name its own"
        );
    }

    #[test]
    fn a_resolved_style_keeps_its_sizes_in_sp_for_the_framework_to_scale() {
        let style = WearTextStyle::LABEL_MEDIUM.resolve(Color::WHITE);
        assert_eq!(style.span_style.font_size, TextUnit::Sp(15.0));
        assert_eq!(style.paragraph_style.line_height, TextUnit::Sp(18.0));
        assert_eq!(style.span_style.font_weight, Some(FontWeight(500)));
        assert_eq!(style.span_style.letter_spacing, TextUnit::Sp(0.4));
    }

    #[test]
    fn a_resolved_style_asks_for_the_wear_line_box_rule() {
        let style = WearTextStyle::TITLE_MEDIUM.resolve(Color::WHITE);
        let policy = style
            .paragraph_style
            .line_height_style
            .expect("a Wear style names its line-height policy");
        assert_eq!(policy.alignment, LineHeightAlignment::Center);
        assert_eq!(policy.trim, LineHeightTrim::None);
        assert_eq!(policy.mode, LineHeightMode::Minimum);
        assert_eq!(
            style
                .paragraph_style
                .platform_style
                .and_then(|platform| platform.include_font_padding),
            Some(false)
        );
    }
}
